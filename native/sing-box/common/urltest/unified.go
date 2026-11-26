package urltest

import (
	"context"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/http/httptrace"
	"sync/atomic"
	"time"
)

type unifiedDelayKey struct{}

// ErrUnifiedConnectionNotReused is returned when unified delay mode is enabled
// but the second probe did not reuse the warmed connection.
var ErrUnifiedConnectionNotReused = errors.New("unified delay: connection not reused")

// DelayMeasurement captures both warm-up and reused-connection probe timing for diagnostics.
type DelayMeasurement struct {
	LatencyMs   int32
	FirstMs     int32
	SecondMs    int32
	Reused      bool
	FirstClosed bool
	Unified     bool
}

// Policy belongs to a box/request context, never a process-wide switch: test
// instances and the running VPN may have different settings during a reload.
func WithUnifiedDelay(ctx context.Context, enabled bool) context.Context {
	return context.WithValue(ctx, unifiedDelayKey{}, enabled)
}

func UnifiedDelayPolicy(ctx context.Context) (bool, bool) {
	value, ok := ctx.Value(unifiedDelayKey{}).(bool)
	return value, ok
}

// MeasureDelay uses the same deadline for warm-up and measurement. When unified
// is enabled, the second probe must succeed on a reused connection; timeout,
// failure, or redial returns an error instead of silently falling back to cold
// handshake latency.
func MeasureDelay(ctx context.Context, client *http.Client, request *http.Request, unified bool) (int32, error) {
	result, err := MeasureDelayDetail(ctx, client, request, unified)
	return result.LatencyMs, err
}

func MeasureDelayDetail(ctx context.Context, client *http.Client, request *http.Request, unified bool) (DelayMeasurement, error) {
	result := DelayMeasurement{Unified: unified}
	request = request.Clone(ctx)
	started := time.Now()
	resp, err := client.Do(request)
	if err != nil {
		return result, err
	}
	_, copyErr := io.Copy(io.Discard, io.LimitReader(resp.Body, 65536))
	_ = resp.Body.Close()
	if copyErr != nil {
		return result, copyErr
	}
	if resp.StatusCode >= 500 {
		return result, fmt.Errorf("HTTP error %d", resp.StatusCode)
	}
	first := time.Since(started)
	result.FirstMs = clampPositiveMs(first)
	result.FirstClosed = resp.Close
	measured := first
	if unified && !resp.Close {
		if err := ctx.Err(); err != nil {
			return result, err
		}
		var reused atomic.Bool
		trace := &httptrace.ClientTrace{GotConn: func(info httptrace.GotConnInfo) { reused.Store(info.Reused) }}
		second := request.Clone(httptrace.WithClientTrace(ctx, trace))
		started = time.Now()
		resp, err = client.Do(second)
		if err != nil {
			return result, err
		}
		_, copyErr = io.Copy(io.Discard, io.LimitReader(resp.Body, 65536))
		_ = resp.Body.Close()
		if copyErr != nil {
			return result, copyErr
		}
		if resp.StatusCode >= 500 {
			return result, fmt.Errorf("HTTP error %d", resp.StatusCode)
		}
		secondDuration := time.Since(started)
		result.SecondMs = clampPositiveMs(secondDuration)
		result.Reused = reused.Load()
		if !result.Reused {
			return result, ErrUnifiedConnectionNotReused
		}
		measured = secondDuration
	}
	result.LatencyMs = clampPositiveMs(measured)
	return result, nil
}

func clampPositiveMs(d time.Duration) int32 {
	ms := int32(d.Milliseconds())
	if ms <= 0 {
		return 1
	}
	return ms
}
