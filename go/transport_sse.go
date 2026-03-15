package haiai

import (
	"bufio"
	"context"
	"encoding/json"
	"fmt"
	"log"
	"net/http"
	"strings"
	"time"
)

// SSEConnection represents an active SSE connection to HAI.
type SSEConnection struct {
	client *Client
	events chan AgentEvent
	done   chan struct{}
	cancel context.CancelFunc
}

// Events returns the channel that receives AgentEvent values from the server.
func (s *SSEConnection) Events() <-chan AgentEvent {
	return s.events
}

// Close terminates the SSE connection.
func (s *SSEConnection) Close() {
	s.cancel()
	<-s.done // wait for goroutine to finish
}

// ConnectSSE establishes an SSE connection to HAI for receiving benchmark jobs.
//
// The returned SSEConnection provides an Events() channel that emits AgentEvent values.
// Call Close() to terminate the connection.
//
// Uses endpoint: GET /api/v1/agents/connect
func (c *Client) ConnectSSE(ctx context.Context) (*SSEConnection, error) {
	ctx, cancel := context.WithCancel(ctx)

	url := c.endpoint + "/api/v1/agents/connect"
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		cancel()
		return nil, wrapError(ErrTransport, err, "failed to create SSE request")
	}

	if err := c.setAuthHeaders(req); err != nil {
		cancel()
		return nil, wrapError(ErrTransport, err, "failed to authenticate SSE request")
	}
	req.Header.Set("Accept", "text/event-stream")
	req.Header.Set("Cache-Control", "no-cache")

	// Use a client without timeout for long-lived SSE connections
	sseHTTPClient := &http.Client{
		// No timeout -- SSE connections are long-lived.
		// Context cancellation handles cleanup.
	}

	resp, err := sseHTTPClient.Do(req)
	if err != nil {
		cancel()
		return nil, wrapError(ErrTransport, err, "SSE connection failed")
	}

	if resp.StatusCode != http.StatusOK {
		cancel()
		resp.Body.Close()
		return nil, newError(ErrTransport, "SSE connection returned status %d", resp.StatusCode)
	}

	conn := &SSEConnection{
		client: c,
		events: make(chan AgentEvent, 16),
		done:   make(chan struct{}),
		cancel: cancel,
	}

	go conn.readLoop(resp)

	return conn, nil
}

// readLoop reads SSE events from the HTTP response body.
func (s *SSEConnection) readLoop(resp *http.Response) {
	defer resp.Body.Close()
	defer close(s.done)
	defer close(s.events)

	scanner := bufio.NewScanner(resp.Body)

	var eventType string
	var dataLines []string

	for scanner.Scan() {
		line := scanner.Text()

		if line == "" {
			// Empty line = end of event
			if len(dataLines) > 0 {
				data := strings.Join(dataLines, "\n")
				event := parseSSEEvent(eventType, data)
				if event != nil {
					select {
					case s.events <- *event:
					default:
						log.Printf("[haiai] WARNING: SSE event dropped (channel full, buffer=%d): type=%s", cap(s.events), event.Type)
					}
				}
			}
			eventType = ""
			dataLines = nil
			continue
		}

		if strings.HasPrefix(line, "event:") {
			eventType = strings.TrimSpace(strings.TrimPrefix(line, "event:"))
		} else if strings.HasPrefix(line, "data:") {
			dataLines = append(dataLines, strings.TrimSpace(strings.TrimPrefix(line, "data:")))
		}
		// Ignore "id:", "retry:", and comment lines (starting with ":")
	}
}

// parseSSEEvent parses an SSE event into an AgentEvent.
func parseSSEEvent(eventType, data string) *AgentEvent {
	if data == "" {
		return nil
	}

	var event AgentEvent
	if err := json.Unmarshal([]byte(data), &event); err != nil {
		// If we can't parse the full JSON, at least set the type
		event.Type = eventType
		return &event
	}

	// Override type from the SSE event: field if present
	if eventType != "" {
		event.Type = eventType
	}

	return &event
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
