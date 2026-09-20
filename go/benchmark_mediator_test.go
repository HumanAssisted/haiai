package haiai

import (
	"context"
	"encoding/json"
	"os"
	"reflect"
	"testing"
)

type benchmarkReplyCapture struct {
	*mockFFIClient
	payload map[string]interface{}
}

func (m *benchmarkReplyCapture) SubmitResponse(params string) (json.RawMessage, error) {
	if err := json.Unmarshal([]byte(params), &m.payload); err != nil {
		return nil, err
	}
	return json.RawMessage(`{"success":true}`), nil
}

func TestSDKMediatorSharedContract(t *testing.T) {
	data, err := os.ReadFile("../fixtures/benchmark_mediator_contract.json")
	if err != nil {
		t.Fatal(err)
	}
	var fixture struct {
		Event    AgentEvent             `json:"event"`
		Response ModerationResponse     `json:"response"`
		Expected map[string]interface{} `json:"ffi_submit_response"`
	}
	if err := json.Unmarshal(data, &fixture); err != nil {
		t.Fatal(err)
	}
	cl, _ := newMockSSEClient(t, []AgentEvent{fixture.Event})
	conn, err := cl.ConnectSSE(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	defer conn.Close()
	event := <-conn.Events()
	var actualMetadata, expectedMetadata interface{}
	if err := json.Unmarshal(event.Config.Metadata, &actualMetadata); err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(fixture.Event.Config.Metadata, &expectedMetadata); err != nil {
		t.Fatal(err)
	}
	if event.JobID != fixture.Event.JobID || event.Config.RunID != fixture.Event.Config.RunID || !reflect.DeepEqual(actualMetadata, expectedMetadata) {
		t.Fatal("transport dropped the completion binding")
	}
	mock := &benchmarkReplyCapture{mockFFIClient: newMockFFIClient("http://localhost", "test-jacs-id", "JACS test:123:sig")}
	cl.ffi = mock
	if _, err := cl.SubmitResponse(context.Background(), event.JobID, fixture.Response); err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(mock.payload, fixture.Expected) {
		t.Fatalf("Rust binding contract mismatch: %#v", mock.payload)
	}
}

func TestMediatorRegistrationOptIn(t *testing.T) {
	data, err := os.ReadFile("../fixtures/benchmark_mediator_contract.json")
	if err != nil {
		t.Fatal(err)
	}
	var fixture struct {
		Registration json.RawMessage `json:"registration"`
	}
	if err := json.Unmarshal(data, &fixture); err != nil {
		t.Fatal(err)
	}
	var opts RegisterOptions
	if err := json.Unmarshal(fixture.Registration, &opts); err != nil {
		t.Fatal(err)
	}
	if opts.IsMediator == nil || !*opts.IsMediator {
		t.Fatal("missing mediator opt-in")
	}
	encoded, err := json.Marshal(opts)
	if err != nil {
		t.Fatal(err)
	}
	var expected, actual interface{}
	if err := json.Unmarshal(fixture.Registration, &expected); err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(encoded, &actual); err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(actual, expected) {
		t.Fatal("registration fields changed")
	}
}
