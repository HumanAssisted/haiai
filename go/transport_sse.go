package haiai

import (
	"context"
	"encoding/json"
	"fmt"
	"log"
	"time"
)

// SSEConnection represents an active SSE connection to HAI via FFI.
type SSEConnection struct {
	client   *Client
	handleID uint64
	events   chan AgentEvent
	done     chan struct{}
	cancel   context.CancelFunc
}

// Events returns the channel that receives AgentEvent values from the server.
func (s *SSEConnection) Events() <-chan AgentEvent {
	return s.events
}

// Close terminates the SSE connection.
func (s *SSEConnection) Close() {
	s.cancel()
	s.client.ffi.SSEClose(s.handleID)
	<-s.done
}

// ConnectSSE establishes an SSE connection to HAI via FFI.
//
// The returned SSEConnection provides an Events() channel that emits AgentEvent values.
// Call Close() to terminate the connection.
func (c *Client) ConnectSSE(ctx context.Context) (*SSEConnection, error) {
	handleID, err := c.ffi.ConnectSSE()
	if err != nil {
		return nil, mapFFIErr(err)
	}

	ctx, cancel := context.WithCancel(ctx)
	conn := &SSEConnection{
		client:   c,
		handleID: handleID,
		events:   make(chan AgentEvent, 16),
		done:     make(chan struct{}),
		cancel:   cancel,
	}

	go conn.readLoop(ctx)

	return conn, nil
}

func (s *SSEConnection) readLoop(ctx context.Context) {
	defer close(s.done)
	defer close(s.events)

	for {
		select {
		case <-ctx.Done():
			return
		default:
		}

		raw, err := s.client.ffi.SSENextEvent(s.handleID)
		if err != nil {
			log.Printf("[haiai] SSE event error: %v", err)
			return
		}
		if raw == nil {
			// Connection closed by server
			return
		}

		var event AgentEvent
		if err := json.Unmarshal(raw, &event); err != nil {
			log.Printf("[haiai] SSE unmarshal error: %v", err)
			continue
		}

		select {
		case s.events <- event:
		case <-ctx.Done():
			return
		default:
			log.Printf("[haiai] WARNING: SSE event dropped (channel full, buffer=%d): type=%s", cap(s.events), event.Type)
		}
	}
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
