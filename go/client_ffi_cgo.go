//go:build cgo

package haiai

import "github.com/HumanAssisted/haiai-go/ffi"

// newFFIClientFromConfig creates a real FFI client from the config JSON.
// This version is compiled when CGo is enabled and libhaiigo is available.
func newFFIClientFromConfig(configJSON string) (FFIClient, error) {
	return ffi.NewClient(configJSON)
}
