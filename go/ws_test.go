package haiai

import (
	"context"
	"encoding/json"
	"fmt"
	"sync"
	"testing"
	"time"
)

// mockWSFFIClient wraps mockFFIClient and adds WS handle simulation.
type mockWSFFIClient struct {
	*mockFFIClient

	mu         sync.Mutex
	events     []json.RawMessage
	eventIdx   int
	closed     bool
	closeCalls int
	nextErr    error
}

func (m *mockWSFFIClient) ConnectWS() (uint64, error) {
	return 1, nil
}

func (m *mockWSFFIClient) WSNextEvent(handleID uint64) (json.RawMessage, error) {
	m.mu.Lock()
	defer m.mu.Unlock()

	if m.closed {
		return nil, nil
	}
	if m.nextErr != nil {
		return nil, m.nextErr
	}
	if m.eventIdx >= len(m.events) {
		return nil, nil
	}
	raw := m.events[m.eventIdx]
	m.eventIdx++
	return raw, nil
}

func (m *mockWSFFIClient) WSClose(handleID uint64) {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.closed = true
	m.closeCalls++
}

func newMockWSClient(t *testing.T, events []AgentEvent) (*Client, *mockWSFFIClient) {
	t.Helper()

	var rawEvents []json.RawMessage
	for _, e := range events {
		data, err := json.Marshal(e)
		if err != nil {
			t.Fatalf("marshal event: %v", err)
		}
		rawEvents = append(rawEvents, json.RawMessage(data))
	}

	mock := &mockWSFFIClient{
		mockFFIClient: newMockFFIClient("http://localhost", "test-jacs-id", "JACS test:123:sig"),
		events:        rawEvents,
	}

	cl := &Client{
		ffi: mock,
	}
	return cl, mock
}

func TestWSConnection(t *testing.T) {
	events := []AgentEvent{
		{Type: "connected", AgentID: "a-1", AgentName: "Test Agent"},
		{Type: "benchmark_job", JobID: "ws-job-1", ScenarioID: "scenario-ws", Config: &BenchmarkJobConfig{
			RunID:        "run-ws",
			ScenarioName: "WS Test",
			Conversation: []ConversationTurn{
				{Speaker: "Bob", Message: "Hi", TurnNumber: 1},
			},
			TimeoutSecs: 30,
		}},
		{Type: "disconnect", Reason: "test done"},
	}

	cl, mock := newMockWSClient(t, events)

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	conn, err := cl.ConnectWS(ctx)
	if err != nil {
		t.Fatalf("ConnectWS: %v", err)
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
	if received[1].JobID != "ws-job-1" {
		t.Errorf("expected job_id 'ws-job-1', got '%s'", received[1].JobID)
	}
	if !mock.closed {
		t.Error("expected WS handle to be closed after stream completion")
	}
	if mock.closeCalls != 1 {
		t.Errorf("expected WS handle to close once, got %d", mock.closeCalls)
	}
}

func TestWSConnectionError(t *testing.T) {
	mock := &mockWSFFIClient{
		mockFFIClient: newMockFFIClient("http://localhost", "test-jacs-id", "JACS test:123:sig"),
	}
	cl := &Client{
		ffi: &failConnectWSMock{mockWSFFIClient: mock},
	}

	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()

	_, err := cl.ConnectWS(ctx)
	if err == nil {
		t.Fatal("expected error from ConnectWS")
	}
}

type failConnectWSMock struct {
	*mockWSFFIClient
}

func (m *failConnectWSMock) ConnectWS() (uint64, error) {
	return 0, fmt.Errorf("connection refused")
}

func TestWSConnectWithHandler(t *testing.T) {
	events := []AgentEvent{
		{Type: "connected", AgentID: "a-1"},
		{Type: "benchmark_job", JobID: "handler-job"},
		{Type: "disconnect", Reason: "done"},
	}

	cl, _ := newMockWSClient(t, events)

	var handledJobs []string

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	err := cl.ConnectWSWithHandler(ctx, func(_ context.Context, _ *WSConnection, event AgentEvent) error {
		handledJobs = append(handledJobs, event.JobID)
		return nil
	})
	if err != nil {
		t.Fatalf("ConnectWSWithHandler: %v", err)
	}

	if len(handledJobs) != 1 || handledJobs[0] != "handler-job" {
		t.Errorf("expected ['handler-job'], got %v", handledJobs)
	}
}

func TestWSSendJobResponseReturnsError(t *testing.T) {
	// SendJobResponse is not supported via FFI; it should return an error.
	ws := &WSConnection{
		client: &Client{},
	}

	err := ws.SendJobResponse("job-1", ModerationResponse{Message: "test"})
	if err == nil {
		t.Fatal("expected error from SendJobResponse via FFI")
	}
}

func TestWSConnectionClosesHandleOnReadError(t *testing.T) {
	mock := &mockWSFFIClient{
		mockFFIClient: newMockFFIClient("http://localhost", "test-jacs-id", "JACS test:123:sig"),
		nextErr:       fmt.Errorf("stream failed"),
	}
	cl := &Client{ffi: mock}

	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()

	conn, err := cl.ConnectWS(ctx)
	if err != nil {
		t.Fatalf("ConnectWS: %v", err)
	}

	for range conn.Events() {
		t.Fatal("expected no events when WSNextEvent returns an error")
	}

	if !mock.closed {
		t.Fatal("expected WS handle to close after read error")
	}
	if mock.closeCalls != 1 {
		t.Fatalf("expected WS handle to close once, got %d", mock.closeCalls)
	}
}
