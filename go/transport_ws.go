package haiai

import (
	"context"
	"fmt"
)

// WSConnection represents an active WebSocket connection to HAI via FFI.
type WSConnection struct {
	client    *Client
	handleID  uint64
	events    chan AgentEvent
	done      chan struct{}
	cancel    context.CancelFunc
	lifecycle *streamLifecycle
}

// Events returns the channel that receives AgentEvent values from the server.
func (ws *WSConnection) Events() <-chan AgentEvent {
	return ws.events
}

// Close terminates the WebSocket connection.
func (ws *WSConnection) Close() {
	if ws.lifecycle != nil {
		ws.lifecycle.close()
		return
	}
	ws.cancel()
	ws.client.ffi.WSClose(ws.handleID)
	<-ws.done
}

// SendJobResponse sends a job response directly over the WebSocket.
// TODO: Bidirectional WS send is not yet supported via FFI. Use client.SubmitResponse() instead.
func (ws *WSConnection) SendJobResponse(jobID string, response ModerationResponse) error {
	return newError(ErrTransport, "SendJobResponse is not supported via FFI; use client.SubmitResponse() instead")
}

// ConnectWS establishes a WebSocket connection to HAI via FFI.
//
// The returned WSConnection provides an Events() channel that emits AgentEvent values.
// Call Close() to terminate the connection.
//
// Uses endpoint: GET /ws/agent/connect (upgraded to WebSocket).
func (c *Client) ConnectWS(ctx context.Context) (*WSConnection, error) {
	lifecycle, err := openStreamLifecycle(
		ctx,
		c.ffi.ConnectWS,
		c.ffi.WSNextEvent,
		c.ffi.WSClose,
		"WS",
	)
	if err != nil {
		return nil, err
	}

	conn := &WSConnection{
		client:    c,
		handleID:  lifecycle.handleID,
		events:    lifecycle.events,
		done:      lifecycle.done,
		cancel:    lifecycle.cancel,
		lifecycle: lifecycle,
	}

	return conn, nil
}

// ConnectWSWithHandler connects via WebSocket and dispatches benchmark jobs to a handler.
// Blocks until the context is cancelled or the connection is closed.
func (c *Client) ConnectWSWithHandler(ctx context.Context, handler func(context.Context, *WSConnection, AgentEvent) error) error {
	conn, err := c.ConnectWS(ctx)
	if err != nil {
		return err
	}
	defer conn.Close()

	for event := range conn.Events() {
		switch event.Type {
		case "benchmark_job":
			if err := handler(ctx, conn, event); err != nil {
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
