package haiai

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"strings"
	"sync"
	"time"

	"github.com/gorilla/websocket"
)

// WSConnection represents an active WebSocket connection to HAI.
type WSConnection struct {
	client *Client
	conn   *websocket.Conn
	events chan AgentEvent
	done   chan struct{}
	cancel context.CancelFunc

	mu sync.Mutex // protects conn writes
}

// Events returns the channel that receives AgentEvent values from the server.
func (ws *WSConnection) Events() <-chan AgentEvent {
	return ws.events
}

// Close terminates the WebSocket connection.
func (ws *WSConnection) Close() {
	ws.cancel()

	ws.mu.Lock()
	if ws.conn != nil {
		ws.conn.WriteMessage(
			websocket.CloseMessage,
			websocket.FormatCloseMessage(websocket.CloseNormalClosure, ""),
		)
	}
	ws.mu.Unlock()

	<-ws.done
}

// SendJobResponse sends a job response directly over the WebSocket.
// This is an alternative to using the HTTP POST endpoint.
func (ws *WSConnection) SendJobResponse(jobID string, response ModerationResponse) error {
	msg := struct {
		Type     string             `json:"type"`
		JobID    string             `json:"job_id"`
		Response ModerationResponse `json:"response"`
	}{
		Type:     "job_response",
		JobID:    jobID,
		Response: response,
	}

	data, err := json.Marshal(msg)
	if err != nil {
		return wrapError(ErrTransport, err, "failed to marshal job response")
	}

	ws.mu.Lock()
	defer ws.mu.Unlock()

	if ws.conn == nil {
		return newError(ErrTransport, "WebSocket connection closed")
	}

	return ws.conn.WriteMessage(websocket.TextMessage, data)
}

// SendPong sends a pong response to a heartbeat ping.
func (ws *WSConnection) sendPong(timestamp int64) error {
	msg := struct {
		Type      string `json:"type"`
		Timestamp int64  `json:"timestamp"`
	}{
		Type:      "pong",
		Timestamp: timestamp,
	}

	data, err := json.Marshal(msg)
	if err != nil {
		return err
	}

	ws.mu.Lock()
	defer ws.mu.Unlock()

	if ws.conn == nil {
		return newError(ErrTransport, "WebSocket connection closed")
	}

	return ws.conn.WriteMessage(websocket.TextMessage, data)
}

// ConnectWS establishes a WebSocket connection to HAI for real-time communication.
//
// JACS authentication is handled via HTTP upgrade headers (Authorization header).
// No post-connection handshake message is required — this matches the Python SDK
// behavior and the server's resolve_agent_for_connection() which extracts JACS
// credentials from the upgrade request headers.
//
// Uses endpoint: GET /ws/agent/connect (upgraded to WebSocket).
func (c *Client) ConnectWS(ctx context.Context) (*WSConnection, error) {
	ctx, cancel := context.WithCancel(ctx)

	// Build WebSocket URL (http -> ws, https -> wss)
	wsURL := c.endpoint + "/ws/agent/connect"
	wsURL = strings.Replace(wsURL, "https://", "wss://", 1)
	wsURL = strings.Replace(wsURL, "http://", "ws://", 1)

	// Build auth headers via CryptoBackend
	authHeader, err := c.buildAuthHeader()
	if err != nil {
		cancel()
		return nil, wrapError(ErrTransport, err, "failed to authenticate WebSocket request")
	}
	requestHeader := http.Header{}
	requestHeader.Set("Authorization", authHeader)

	dialer := websocket.Dialer{
		HandshakeTimeout: 10 * time.Second,
	}

	wsConn, resp, err := dialer.DialContext(ctx, wsURL, requestHeader)
	if err != nil {
		cancel()
		if resp != nil {
			return nil, newError(ErrTransport, "WebSocket upgrade failed with status %d", resp.StatusCode)
		}
		return nil, wrapError(ErrTransport, err, "WebSocket connection failed")
	}

	conn := &WSConnection{
		client: c,
		conn:   wsConn,
		events: make(chan AgentEvent, 16),
		done:   make(chan struct{}),
		cancel: cancel,
	}

	go conn.readLoop(ctx)

	return conn, nil
}

// readLoop reads messages from the WebSocket connection.
func (ws *WSConnection) readLoop(ctx context.Context) {
	defer close(ws.done)
	defer close(ws.events)
	defer func() {
		ws.mu.Lock()
		if ws.conn != nil {
			ws.conn.Close()
			ws.conn = nil
		}
		ws.mu.Unlock()
	}()

	for {
		select {
		case <-ctx.Done():
			return
		default:
		}

		_, message, err := ws.conn.ReadMessage()
		if err != nil {
			if websocket.IsCloseError(err, websocket.CloseNormalClosure, websocket.CloseGoingAway) {
				return
			}
			// Context cancelled or other error
			return
		}

		var event AgentEvent
		if err := json.Unmarshal(message, &event); err != nil {
			continue
		}

		// Auto-reply to heartbeats
		if event.Type == "heartbeat" {
			ws.sendPong(event.Timestamp)
		}

		select {
		case ws.events <- event:
		default:
			// Channel full, drop event
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
