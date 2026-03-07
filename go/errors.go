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
)

// Error represents errors from HAI SDK operations.
type Error struct {
	Kind    ErrorKind
	Message string
	Err     error // underlying error, if any
}

func (e *Error) Error() string {
	if e.Err != nil {
		return fmt.Sprintf("%s: %v", e.Message, e.Err)
	}
	return e.Message
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

func wrapError(kind ErrorKind, err error, format string, args ...interface{}) *Error {
	return &Error{
		Kind:    kind,
		Message: fmt.Sprintf(format, args...),
		Err:     err,
	}
}
