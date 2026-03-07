package haiai

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/gorilla/websocket"
)

func TestWSConnection(t *testing.T) {
	upgrader := websocket.Upgrader{
		CheckOrigin: func(_ *http.Request) bool { return true },
	}

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/ws/agent/connect" {
			t.Errorf("unexpected path: %s", r.URL.Path)
			w.WriteHeader(http.StatusNotFound)
			return
		}

		// Verify JACS auth header
		auth := r.Header.Get("Authorization")
		if !strings.HasPrefix(auth, "JACS ") {
			w.WriteHeader(http.StatusUnauthorized)
			return
		}

		conn, err := upgrader.Upgrade(w, r, nil)
		if err != nil {
			t.Errorf("upgrade failed: %v", err)
			return
		}
		defer conn.Close()

		// Send connected event
		connEvent := AgentEvent{Type: "connected", AgentID: "a-1", AgentName: "Test Agent"}
		data, _ := json.Marshal(connEvent)
		conn.WriteMessage(websocket.TextMessage, data)

		// Send benchmark job
		jobEvent := AgentEvent{
			Type:       "benchmark_job",
			JobID:      "ws-job-1",
			ScenarioID: "scenario-ws",
			Config: &BenchmarkJobConfig{
				RunID:        "run-ws",
				ScenarioName: "WS Test",
				Conversation: []ConversationTurn{
					{Speaker: "Bob", Message: "Hi", TurnNumber: 1},
				},
				TimeoutSecs: 30,
			},
		}
		data, _ = json.Marshal(jobEvent)
		conn.WriteMessage(websocket.TextMessage, data)

		// Wait for job response from client
		_, msg, err := conn.ReadMessage()
		if err != nil {
			return
		}
		var wsMsg struct {
			Type string `json:"type"`
		}
		json.Unmarshal(msg, &wsMsg)
		if wsMsg.Type != "job_response" && wsMsg.Type != "pong" {
			// Accept pong or job_response
		}

		// Send disconnect
		disconnectEvent := AgentEvent{Type: "disconnect", Reason: "test done"}
		data, _ = json.Marshal(disconnectEvent)
		conn.WriteMessage(websocket.TextMessage, data)

		time.Sleep(100 * time.Millisecond)
	}))
	defer server.Close()

	cl, _ := newTestClient(t, server.URL)

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	conn, err := cl.ConnectWS(ctx)
	if err != nil {
		t.Fatalf("ConnectWS: %v", err)
	}
	defer conn.Close()

	var received []AgentEvent
	for event := range conn.Events() {
		received = append(received, event)
		if event.Type == "benchmark_job" {
			// Send a response via WebSocket
			err := conn.SendJobResponse(event.JobID, ModerationResponse{
				Message: "I understand your concern.",
			})
			if err != nil {
				t.Errorf("SendJobResponse: %v", err)
			}
		}
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
}

func TestWSHeartbeatAutoPong(t *testing.T) {
	upgrader := websocket.Upgrader{
		CheckOrigin: func(_ *http.Request) bool { return true },
	}

	pongReceived := make(chan bool, 1)

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		conn, err := upgrader.Upgrade(w, r, nil)
		if err != nil {
			return
		}
		defer conn.Close()

		// Send connected
		connEvent := AgentEvent{Type: "connected", AgentID: "a-1"}
		data, _ := json.Marshal(connEvent)
		conn.WriteMessage(websocket.TextMessage, data)

		// Send heartbeat
		hb := AgentEvent{Type: "heartbeat", Timestamp: 1700000000}
		data, _ = json.Marshal(hb)
		conn.WriteMessage(websocket.TextMessage, data)

		// Wait for pong response
		_, msg, err := conn.ReadMessage()
		if err != nil {
			return
		}

		var pong struct {
			Type      string `json:"type"`
			Timestamp int64  `json:"timestamp"`
		}
		if err := json.Unmarshal(msg, &pong); err == nil && pong.Type == "pong" {
			pongReceived <- true
		}

		// Close
		conn.WriteMessage(websocket.CloseMessage,
			websocket.FormatCloseMessage(websocket.CloseNormalClosure, ""))
	}))
	defer server.Close()

	cl, _ := newTestClient(t, server.URL)

	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()

	conn, err := cl.ConnectWS(ctx)
	if err != nil {
		t.Fatalf("ConnectWS: %v", err)
	}
	defer conn.Close()

	// Drain events
	go func() {
		for range conn.Events() {
		}
	}()

	select {
	case <-pongReceived:
		// Auto-pong worked
	case <-time.After(2 * time.Second):
		t.Fatal("timed out waiting for auto-pong")
	}
}

func TestWSConnectionURLConversion(t *testing.T) {
	// Verify the http->ws URL conversion logic by checking ConnectWS
	// fails gracefully with a bad server (no actual WS server)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusBadRequest)
	}))
	defer server.Close()

	cl, _ := newTestClient(t, server.URL)

	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()

	_, err := cl.ConnectWS(ctx)
	if err == nil {
		t.Fatal("expected error for non-WS server")
	}
}

func TestWSConnectWithHandler(t *testing.T) {
	upgrader := websocket.Upgrader{
		CheckOrigin: func(_ *http.Request) bool { return true },
	}

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		conn, err := upgrader.Upgrade(w, r, nil)
		if err != nil {
			return
		}
		defer conn.Close()

		// Send connected
		event := AgentEvent{Type: "connected", AgentID: "a-1"}
		data, _ := json.Marshal(event)
		conn.WriteMessage(websocket.TextMessage, data)

		// Send benchmark job
		jobEvent := AgentEvent{Type: "benchmark_job", JobID: "handler-job"}
		data, _ = json.Marshal(jobEvent)
		conn.WriteMessage(websocket.TextMessage, data)

		// Read job response from handler
		conn.ReadMessage()

		// Send disconnect
		disconnectEvent := AgentEvent{Type: "disconnect", Reason: "done"}
		data, _ = json.Marshal(disconnectEvent)
		conn.WriteMessage(websocket.TextMessage, data)

		time.Sleep(100 * time.Millisecond)
	}))
	defer server.Close()

	cl, _ := newTestClient(t, server.URL)

	var handledJobs []string

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	err := cl.ConnectWSWithHandler(ctx, func(_ context.Context, wsConn *WSConnection, event AgentEvent) error {
		handledJobs = append(handledJobs, event.JobID)
		return wsConn.SendJobResponse(event.JobID, ModerationResponse{
			Message: "Handled via WS",
		})
	})
	if err != nil {
		t.Fatalf("ConnectWSWithHandler: %v", err)
	}

	if len(handledJobs) != 1 || handledJobs[0] != "handler-job" {
		t.Errorf("expected ['handler-job'], got %v", handledJobs)
	}
}

func TestWSSendJobResponseClosed(t *testing.T) {
	ws := &WSConnection{
		conn: nil, // simulate closed connection
	}

	err := ws.SendJobResponse("job-1", ModerationResponse{Message: "test"})
	if err == nil {
		t.Fatal("expected error when conn is nil")
	}
}
