package haiai

import "testing"

// TestCryptoBackendIsJacs verifies that the module-level cryptoBackend is
// always a *jacsBackend. This guards against future regressions that could
// re-introduce a fallback backend.
func TestCryptoBackendIsJacs(t *testing.T) {
	if _, ok := cryptoBackend.(*jacsBackend); !ok {
		t.Fatalf("expected *jacsBackend, got %T", cryptoBackend)
	}
}
