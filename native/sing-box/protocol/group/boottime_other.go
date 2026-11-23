//go:build !linux

package group

import "time"

var fallbackOrigin = time.Now()

func elapsedBootTime() time.Duration {
	return time.Since(fallbackOrigin)
}
