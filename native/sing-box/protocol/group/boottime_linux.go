//go:build linux

package group

import (
	"time"

	"golang.org/x/sys/unix"
)

var fallbackOrigin = time.Now()

func elapsedBootTime() time.Duration {
	var ts unix.Timespec
	if err := unix.ClockGettime(unix.CLOCK_BOOTTIME, &ts); err == nil {
		return time.Duration(ts.Sec)*time.Second + time.Duration(ts.Nsec)*time.Nanosecond
	}
	return time.Since(fallbackOrigin)
}
