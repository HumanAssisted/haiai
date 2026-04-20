package haiai

import (
	"context"
	"encoding/json"
	"fmt"
	"sync"
	"testing"
	"time"
)

// mockSSEFFIClient wraps mockFFIClient and adds SSE handle simulation.
type mockSSEFFIClient struct {
	*mockFFIClient

	mu         sync.Mutex
	events     []json.RawMessage // queued events to return from SSENextEvent
	eventIdx   int
	closed     bool
	closeCalls int
	nextErr    error
}

func (m *mockSSEFFIClient) ConnectSSE() (uint64, error) {
	return 1, nil
}

func (m *mockSSEFFIClient) SSENextEvent(handleID uint64) (json.RawMessage, error) {
	m.mu.Lock()
	defer m.mu.Unlock()

	if m.closed {
		return nil, nil
	}
	if m.nextErr != nil {
		return nil, m.nextErr
	}
	if m.eventIdx >= len(m.events) {
		// Signal end of stream
		return nil, nil
	}
	raw := m.events[m.eventIdx]
	m.eventIdx++
	return raw, nil
}

func (m *mockSSEFFIClient) SSEClose(handleID uint64) {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.closed = true
	m.closeCalls++
}

func newMockSSEClient(t *testing.T, events []AgentEvent) (*Client, *mockSSEFFIClient) {
	t.Helper()

	var rawEvents []json.RawMessage
	for _, e := range events {
		data, err := json.Marshal(e)
		if err != nil {
			t.Fatalf("marshal event: %v", err)
		}
		rawEvents = append(rawEvents, json.RawMessage(data))
	}

	mock := &mockSSEFFIClient{
		mockFFIClient: newMockFFIClient("http://localhost", "test-jacs-id", "JACS test:123:sig"),
		events:        rawEvents,
	}

	cl := &Client{
		ffi: mock,
	}
	return cl, mock
}

func TestSSEConnection(t *testing.T) {
	events := []AgentEvent{
		{Type: "connected", AgentID: "a-1", AgentName: "Test Agent"},
		{Type: "benchmark_job", JobID: "job-123", ScenarioID: "scenario-1", Config: &BenchmarkJobConfig{
			RunID:        "run-456",
			ScenarioName: "Test Scenario",
			Conversation: []ConversationTurn{
				{Speaker: "Alice", Message: "Hello", TurnNumber: 1},
			},
			TimeoutSecs: 60,
		}},
		{Type: "disconnect", Reason: "test complete"},
	}

	cl, mock := newMockSSEClient(t, events)

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	conn, err := cl.ConnectSSE(ctx)
	if err != nil {
		t.Fatalf("ConnectSSE: %v", err)
	}

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
	if !mock.closed {
		t.Error("expected SSE handle to be closed after stream completion")
	}
	if mock.closeCalls != 1 {
		t.Errorf("expected SSE handle to close once, got %d", mock.closeCalls)
	}
}

func TestSSEConnectionError(t *testing.T) {
	mock := &mockSSEFFIClient{
		mockFFIClient: newMockFFIClient("http://localhost", "test-jacs-id", "JACS test:123:sig"),
	}
	// Override ConnectSSE to return an error
	cl := &Client{
		ffi: &failConnectSSEMock{mockSSEFFIClient: mock},
	}

	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()

	_, err := cl.ConnectSSE(ctx)
	if err == nil {
		t.Fatal("expected error from ConnectSSE")
	}
}

// failConnectSSEMock wraps mockSSEFFIClient but fails ConnectSSE.
type failConnectSSEMock struct {
	*mockSSEFFIClient
}

func (m *failConnectSSEMock) ConnectSSE() (uint64, error) {
	return 0, fmt.Errorf("connection refused")
}

func TestConnectSSEWithHandler(t *testing.T) {
	events := []AgentEvent{
		{Type: "benchmark_job", JobID: "j-1", ScenarioID: "s-1"},
		{Type: "disconnect", Reason: "done"},
	}

	cl, _ := newMockSSEClient(t, events)

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

func TestSSEConnectionClosesHandleOnReadError(t *testing.T) {
	mock := &mockSSEFFIClient{
		mockFFIClient: newMockFFIClient("http://localhost", "test-jacs-id", "JACS test:123:sig"),
		nextErr:       fmt.Errorf("stream failed"),
	}
	cl := &Client{ffi: mock}

	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()

	conn, err := cl.ConnectSSE(ctx)
	if err != nil {
		t.Fatalf("ConnectSSE: %v", err)
	}

	for range conn.Events() {
		t.Fatal("expected no events when SSENextEvent returns an error")
	}

	if !mock.closed {
		t.Fatal("expected SSE handle to close after read error")
	}
	if mock.closeCalls != 1 {
		t.Fatalf("expected SSE handle to close once, got %d", mock.closeCalls)
	}
}
