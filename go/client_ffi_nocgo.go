//go:build !cgo

package haiai

// newFFIClientFromConfig returns an error when CGo is not available.
// Production usage requires CGo with libhaiigo. For testing, inject a mock
// via WithFFIClient().
func newFFIClientFromConfig(configJSON string) (FFIClient, error) {
	return nil, newError(ErrConfigInvalid,
		"FFI client required: build with CGO_ENABLED=1 and libhaiigo, or inject via WithFFIClient()")
}
