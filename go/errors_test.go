package haiai

import (
	"errors"
	"testing"
)

func TestErrorMessage(t *testing.T) {
	err := newError(ErrConnection, "connection failed: %s", "timeout")
	if err.Error() != "connection failed: timeout" {
		t.Errorf("expected 'connection failed: timeout', got '%s'", err.Error())
	}
}

func TestErrorKind(t *testing.T) {
	err := newError(ErrAuthRequired, "auth required")
	if err.Kind != ErrAuthRequired {
		t.Errorf("expected ErrAuthRequired, got %v", err.Kind)
	}
}

func TestWrapError(t *testing.T) {
	inner := errors.New("underlying error")
	err := wrapError(ErrConnection, inner, "operation failed")

	if err.Error() != "operation failed: underlying error" {
		t.Errorf("unexpected message: %s", err.Error())
	}

	if !errors.Is(err, inner) {
		t.Error("wrapped error should unwrap to inner error")
	}
}

func TestWrapErrorNilInner(t *testing.T) {
	err := &Error{
		Kind:    ErrConnection,
		Message: "no inner",
		Err:     nil,
	}

	if err.Error() != "no inner" {
		t.Errorf("expected 'no inner', got '%s'", err.Error())
	}

	if err.Unwrap() != nil {
		t.Error("unwrap should return nil when no inner error")
	}
}
