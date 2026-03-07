package haiai

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"
)

func TestSSEParseEvent(t *testing.T) {
	tests := []struct {
		name      string
		eventType string
		data      string
		wantType  string
		wantNil   bool
	}{
		{
			name:      "benchmark job",
			eventType: "benchmark_job",
			data:      `{"type":"benchmark_job","job_id":"j-1","scenario_id":"s-1"}`,
			wantType:  "benchmark_job",
		},
		{
			name:      "heartbeat",
			eventType: "heartbeat",
			data:      `{"type":"heartbeat","timestamp":1700000000}`,
			wantType:  "heartbeat",
		},
		{
			name:      "connected",
			eventType: "connected",
			data:      `{"type":"connected","agent_id":"a-1","agent_name":"Test Agent"}`,
			wantType:  "connected",
		},
		{
			name:      "disconnect",
			eventType: "disconnect",
			data:      `{"type":"disconnect","reason":"server shutting down"}`,
			wantType:  "disconnect",
		},
		{
			name:    "empty data",
			data:    "",
			wantNil: true,
		},
		{
			name:      "event type overrides json type",
			eventType: "custom_type",
			data:      `{"type":"original_type"}`,
			wantType:  "custom_type",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			event := parseSSEEvent(tt.eventType, tt.data)
			if tt.wantNil {
				if event != nil {
					t.Error("expected nil event")
				}
				return
			}
			if event == nil {
				t.Fatal("expected non-nil event")
			}
			if event.Type != tt.wantType {
				t.Errorf("expected type '%s', got '%s'", tt.wantType, event.Type)
			}
		})
	}
}

func TestSSEConnection(t *testing.T) {
	// Create an SSE server that sends a connected event and a benchmark job
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/api/v1/agents/connect" {
			t.Errorf("unexpected path: %s", r.URL.Path)
			w.WriteHeader(http.StatusNotFound)
			return
		}

		// Verify JACS auth
		auth := r.Header.Get("Authorization")
		if auth == "" {
			w.WriteHeader(http.StatusUnauthorized)
			return
		}

		flusher, ok := w.(http.Flusher)
		if !ok {
			t.Fatal("server does not support flushing")
			return
		}

		w.Header().Set("Content-Type", "text/event-stream")
		w.Header().Set("Cache-Control", "no-cache")
		w.WriteHeader(http.StatusOK)

		// Send connected event
		connEvent := AgentEvent{Type: "connected", AgentID: "a-1", AgentName: "Test Agent"}
		data, _ := json.Marshal(connEvent)
		fmt.Fprintf(w, "event: connected\ndata: %s\n\n", data)
		flusher.Flush()

		// Send benchmark job
		jobEvent := AgentEvent{
			Type:       "benchmark_job",
			JobID:      "job-123",
			ScenarioID: "scenario-1",
			Config: &BenchmarkJobConfig{
				RunID:        "run-456",
				ScenarioName: "Test Scenario",
				Conversation: []ConversationTurn{
					{Speaker: "Alice", Message: "Hello", TurnNumber: 1},
				},
				TimeoutSecs: 60,
			},
		}
		data, _ = json.Marshal(jobEvent)
		fmt.Fprintf(w, "event: benchmark_job\ndata: %s\n\n", data)
		flusher.Flush()

		// Send disconnect
		disconnectEvent := AgentEvent{Type: "disconnect", Reason: "test complete"}
		data, _ = json.Marshal(disconnectEvent)
		fmt.Fprintf(w, "event: disconnect\ndata: %s\n\n", data)
		flusher.Flush()
	}))
	defer server.Close()

	cl, _ := newTestClient(t, server.URL)

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	conn, err := cl.ConnectSSE(ctx)
	if err != nil {
		t.Fatalf("ConnectSSE: %v", err)
	}
	defer conn.Close()

	// Read events
	var received []AgentEvent
	for event := range conn.Events() {
		received = append(received, event)
		if event.Type == "disconnect" {
			break
		}
	}

	if len(received) < 3 {
		t.Fatalf("expected at least 3 events, got %d", len(received))
	}

	if received[0].Type != "connected" {
		t.Errorf("first event should be 'connected', got '%s'", received[0].Type)
	}
	if received[1].Type != "benchmark_job" {
		t.Errorf("second event should be 'benchmark_job', got '%s'", received[1].Type)
	}
	if received[1].JobID != "job-123" {
		t.Errorf("expected job_id 'job-123', got '%s'", received[1].JobID)
	}
	if received[2].Type != "disconnect" {
		t.Errorf("third event should be 'disconnect', got '%s'", received[2].Type)
	}
}

func TestSSEConnectionAuthFailure(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusUnauthorized)
	}))
	defer server.Close()

	cl, _ := newTestClient(t, server.URL)

	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()

	_, err := cl.ConnectSSE(ctx)
	if err == nil {
		t.Fatal("expected error for 401 response")
	}
}

func TestSSEConnectionServerError(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusInternalServerError)
	}))
	defer server.Close()

	cl, _ := newTestClient(t, server.URL)

	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()

	_, err := cl.ConnectSSE(ctx)
	if err == nil {
		t.Fatal("expected error for 500 response")
	}

	sdkErr, ok := err.(*Error)
	if !ok {
		t.Fatalf("expected *Error, got %T", err)
	}
	if sdkErr.Kind != ErrTransport {
		t.Errorf("expected ErrTransport, got %v", sdkErr.Kind)
	}
}

func TestConnectSSEWithHandler(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		flusher, ok := w.(http.Flusher)
		if !ok {
			return
		}

		w.Header().Set("Content-Type", "text/event-stream")
		w.WriteHeader(http.StatusOK)

		// Send a benchmark job
		event := AgentEvent{Type: "benchmark_job", JobID: "j-1", ScenarioID: "s-1"}
		data, _ := json.Marshal(event)
		fmt.Fprintf(w, "event: benchmark_job\ndata: %s\n\n", data)
		flusher.Flush()

		// Send disconnect to end the handler
		disconnect := AgentEvent{Type: "disconnect", Reason: "done"}
		data, _ = json.Marshal(disconnect)
		fmt.Fprintf(w, "event: disconnect\ndata: %s\n\n", data)
		flusher.Flush()
	}))
	defer server.Close()

	cl, _ := newTestClient(t, server.URL)

	var handledJobs []string
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	err := cl.ConnectSSEWithHandler(ctx, func(_ context.Context, event AgentEvent) error {
		handledJobs = append(handledJobs, event.JobID)
		return nil
	})
	if err != nil {
		t.Fatalf("ConnectSSEWithHandler: %v", err)
	}

	if len(handledJobs) != 1 {
		t.Errorf("expected 1 handled job, got %d", len(handledJobs))
	}
	if handledJobs[0] != "j-1" {
		t.Errorf("expected job ID 'j-1', got '%s'", handledJobs[0])
	}
}
