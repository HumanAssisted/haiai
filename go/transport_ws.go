package haiai

import (
	"context"
	"encoding/json"
	"fmt"
	"log"
)

// WSConnection represents an active WebSocket connection to HAI via FFI.
type WSConnection struct {
	client   *Client
	handleID uint64
	events   chan AgentEvent
	done     chan struct{}
	cancel   context.CancelFunc
}

// Events returns the channel that receives AgentEvent values from the server.
func (ws *WSConnection) Events() <-chan AgentEvent {
	return ws.events
}

// Close terminates the WebSocket connection.
func (ws *WSConnection) Close() {
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
	handleID, err := c.ffi.ConnectWS()
	if err != nil {
		return nil, mapFFIErr(err)
	}

	ctx, cancel := context.WithCancel(ctx)
	conn := &WSConnection{
		client:   c,
		handleID: handleID,
		events:   make(chan AgentEvent, 16),
		done:     make(chan struct{}),
		cancel:   cancel,
	}

	go conn.readLoop(ctx)

	return conn, nil
}

func (ws *WSConnection) readLoop(ctx context.Context) {
	defer close(ws.done)
	defer close(ws.events)

	for {
		select {
		case <-ctx.Done():
			return
		default:
		}

		raw, err := ws.client.ffi.WSNextEvent(ws.handleID)
		if err != nil {
			log.Printf("[haiai] WS event error: %v", err)
			return
		}
		if raw == nil {
			// Connection closed by server
			return
		}

		var event AgentEvent
		if err := json.Unmarshal(raw, &event); err != nil {
			log.Printf("[haiai] WS unmarshal error: %v", err)
			continue
		}

		select {
		case ws.events <- event:
		case <-ctx.Done():
			return
		default:
			log.Printf("[haiai] WARNING: WS event dropped (channel full, buffer=%d): type=%s", cap(ws.events), event.Type)
		}
	}
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
