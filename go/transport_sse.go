package haiai

import (
	"context"
	"fmt"
	"time"
)

// SSEConnection represents an active SSE connection to HAI via FFI.
type SSEConnection struct {
	client    *Client
	handleID  uint64
	events    chan AgentEvent
	done      chan struct{}
	cancel    context.CancelFunc
	lifecycle *streamLifecycle
}

// Events returns the channel that receives AgentEvent values from the server.
func (s *SSEConnection) Events() <-chan AgentEvent {
	return s.events
}

// Close terminates the SSE connection.
func (s *SSEConnection) Close() {
	if s.lifecycle != nil {
		s.lifecycle.close()
		return
	}
	s.cancel()
	s.client.ffi.SSEClose(s.handleID)
	<-s.done
}

// ConnectSSE establishes an SSE connection to HAI via FFI.
//
// The returned SSEConnection provides an Events() channel that emits AgentEvent values.
// Call Close() to terminate the connection.
func (c *Client) ConnectSSE(ctx context.Context) (*SSEConnection, error) {
	lifecycle, err := openStreamLifecycle(
		ctx,
		c.ffi.ConnectSSE,
		c.ffi.SSENextEvent,
		c.ffi.SSEClose,
		"SSE",
	)
	if err != nil {
		return nil, err
	}

	conn := &SSEConnection{
		client:    c,
		handleID:  lifecycle.handleID,
		events:    lifecycle.events,
		done:      lifecycle.done,
		cancel:    lifecycle.cancel,
		lifecycle: lifecycle,
	}

	return conn, nil
}

// ConnectSSEWithHandler connects via SSE and dispatches benchmark jobs to a handler.
// This is a convenience method that blocks until the context is cancelled.
//
// The handler receives each AgentEvent of type "benchmark_job".
// Other events (heartbeat, connected, disconnect) are handled automatically.
func (c *Client) ConnectSSEWithHandler(ctx context.Context, handler func(context.Context, AgentEvent) error) error {
	conn, err := c.ConnectSSE(ctx)
	if err != nil {
		return err
	}
	defer conn.Close()

	for event := range conn.Events() {
		switch event.Type {
		case "benchmark_job":
			if err := handler(ctx, event); err != nil {
				return fmt.Errorf("handler error: %w", err)
			}
		case "disconnect":
			return nil
		case "heartbeat", "connected":
			// Handled automatically
		}
	}

	return nil
}

// OnBenchmarkJob is a convenience method that connects via SSE and calls the handler
// for each benchmark job. It reconnects automatically on connection loss with
// exponential backoff (1s, 2s, 4s, ... up to 60s).
//
// The handler receives the job event and should call client.SubmitResponse() with the result.
// Blocks until the context is cancelled.
func (c *Client) OnBenchmarkJob(ctx context.Context, handler func(ctx context.Context, event AgentEvent) error) error {
	reconnectDelay := 1 * time.Second
	maxDelay := 60 * time.Second

	for {
		err := c.ConnectSSEWithHandler(ctx, handler)

		// Check if context was cancelled
		if ctx.Err() != nil {
			return ctx.Err()
		}

		// Reconnect with exponential backoff
		if err != nil {
			select {
			case <-ctx.Done():
				return ctx.Err()
			case <-time.After(reconnectDelay):
				reconnectDelay = reconnectDelay * 2
				if reconnectDelay > maxDelay {
					reconnectDelay = maxDelay
				}
			}
		} else {
			// Connection ended cleanly, reset backoff
			reconnectDelay = 1 * time.Second
		}
	}
}
