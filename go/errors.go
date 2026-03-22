package haiai

import "fmt"

// ErrorKind categorizes HAI SDK errors.
type ErrorKind int

const (
	// ErrConnection indicates a network connection failure.
	ErrConnection ErrorKind = iota
	// ErrRegistration indicates agent registration failed.
	ErrRegistration
	// ErrAuthRequired indicates authentication is required.
	ErrAuthRequired
	// ErrInvalidResponse indicates the server returned an invalid response.
	ErrInvalidResponse
	// ErrKeyNotFound indicates the requested key was not found.
	ErrKeyNotFound
	// ErrConfigNotFound indicates the JACS config file was not found.
	ErrConfigNotFound
	// ErrConfigInvalid indicates the JACS config file is invalid.
	ErrConfigInvalid
	// ErrSigningFailed indicates cryptographic signing failed.
	ErrSigningFailed
	// ErrTimeout indicates a request or operation timed out.
	ErrTimeout
	// ErrTransport indicates a transport-level error (SSE/WebSocket).
	ErrTransport
	// ErrForbidden indicates the server returned 403.
	ErrForbidden
	// ErrNotFound indicates the server returned 404.
	ErrNotFound
	// ErrRateLimited indicates the server returned 429.
	ErrRateLimited
	// ErrJacsNotLoaded indicates a crypto operation was attempted without a JACS agent.
	ErrJacsNotLoaded
	// ErrJacsOpFailed indicates a JACS operation threw an error.
	ErrJacsOpFailed
	// ErrJacsBuildRequired indicates the binary was built without JACS support.
	// Deprecated: Unused after fallback removal. Retained for iota stability.
	ErrJacsBuildRequired
	// ErrVerificationFailed indicates signature verification failed.
	ErrVerificationFailed
	// ErrPrivateKeyMissing indicates the private key file was not found.
	ErrPrivateKeyMissing
	// ErrPrivateKeyPasswordRequired indicates an encrypted key needs a password.
	ErrPrivateKeyPasswordRequired
)

// Error represents errors from HAI SDK operations.
type Error struct {
	Kind    ErrorKind
	Message string
	Action  string // developer-facing hint describing how to fix the issue
	Err     error  // underlying error, if any
}

func (e *Error) Error() string {
	base := e.Message
	if e.Err != nil {
		base = fmt.Sprintf("%s: %v", e.Message, e.Err)
	}
	if e.Action != "" {
		return fmt.Sprintf("%s. %s", base, e.Action)
	}
	return base
}

func (e *Error) Unwrap() error {
	return e.Err
}

func newError(kind ErrorKind, format string, args ...interface{}) *Error {
	return &Error{
		Kind:    kind,
		Message: fmt.Sprintf(format, args...),
	}
}

func newErrorWithAction(kind ErrorKind, action string, format string, args ...interface{}) *Error {
	return &Error{
		Kind:    kind,
		Message: fmt.Sprintf(format, args...),
		Action:  action,
	}
}

func wrapError(kind ErrorKind, err error, format string, args ...interface{}) *Error {
	return &Error{
		Kind:    kind,
		Message: fmt.Sprintf(format, args...),
		Err:     err,
	}
}

func wrapErrorWithAction(kind ErrorKind, err error, action string, format string, args ...interface{}) *Error {
	return &Error{
		Kind:    kind,
		Message: fmt.Sprintf(format, args...),
		Action:  action,
		Err:     err,
	}
}
