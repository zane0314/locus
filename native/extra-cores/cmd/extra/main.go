package main

/*
#include <stdlib.h>

extern int extra_protect(int fd);
*/
import "C"

import (
	"fmt"
	"unsafe"

	"zanebox/engines"
)

func runSafely(fn func() error) (out string) {
	defer func() {
		if r := recover(); r != nil {
			out = fmt.Sprintf("native panic: %v", r)
		}
	}()
	if err := fn(); err != nil {
		return err.Error()
	}
	return ""
}

//export ExtraVersion
func ExtraVersion(kind *C.char) *C.char {
	return C.CString(engines.Version(C.GoString(kind)))
}

//export ExtraValidate
func ExtraValidate(kind *C.char, config *C.char) *C.char {
	return C.CString(runSafely(func() error {
		return engines.Validate(C.GoString(kind), C.GoString(config))
	}))
}

//export ExtraStart
func ExtraStart(kind *C.char, config *C.char) *C.char {
	return C.CString(runSafely(func() error {
		return engines.Start(C.GoString(kind), C.GoString(config), func(fd int) bool {
			return C.extra_protect(C.int(fd)) != 0
		})
	}))
}

//export ExtraStop
func ExtraStop() *C.char {
	return C.CString(runSafely(func() error {
		return engines.Stop()
	}))
}

//export ExtraFree
func ExtraFree(p *C.char) {
	if p != nil {
		C.free(unsafe.Pointer(p))
	}
}

func main() {}
