package libcore

import (
	"net/http"
	"net/http/httptest"
	"sync/atomic"
	"testing"
)

func TestUrlTestSingleDoesNotRetryFallback(t *testing.T) {
	var hits atomic.Int32
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		hits.Add(1)
		w.WriteHeader(http.StatusNoContent)
	}))
	defer srv.Close()

	delay, err := UrlTestSingle(nil, srv.URL, 2000)
	if err != nil {
		t.Fatalf("UrlTestSingle failed: %v", err)
	}
	if delay <= 0 {
		t.Fatalf("expected positive delay, got %d", delay)
	}
	if hits.Load() == 0 {
		t.Fatalf("expected server to be hit")
	}
}
