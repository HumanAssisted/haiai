package haiai

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
)

type initBootstrapRegisterContract struct {
	Method            string `json:"method"`
	Path              string `json:"path"`
	AuthRequired      bool   `json:"auth_required"`
	PublicKeyEncoding string `json:"public_key_encoding"`
}

type registrationOutcomeCase struct {
	Name           string          `json:"name"`
	HTTPStatus     int             `json:"http_status"`
	Response       json.RawMessage `json:"response"`
	ExpectedStatus *string         `json:"expected_status"`
	ExpectedEmail  *string         `json:"expected_email"`
}

type initContractFixture struct {
	RegistrationOutcomes struct {
		Cases []registrationOutcomeCase `json:"cases"`
	} `json:"registration_outcomes"`
	BootstrapRegister        initBootstrapRegisterContract `json:"bootstrap_register"`
	ExistingIdentityRegister struct {
		Response json.RawMessage `json:"response"`
		Cases    []struct {
			Name    string            `json:"name"`
			Request map[string]string `json:"request"`
		} `json:"cases"`
	} `json:"existing_identity_register"`
	PrivateKeyCandidateOrder   []string `json:"private_key_candidate_order"`
	ConfigDiscoveryOrder       []string `json:"config_discovery_order"`
	PrivateKeyPasswordSources  []string `json:"private_key_password_sources"`
	PrivateKeyPasswordStrategy string   `json:"private_key_password_strategy"`
}

type registrationCaptureFFI struct {
	FFIClient // Any unexpected operation (including identity creation) fails.
	payloads  []string
	response  json.RawMessage
	err       error
}

func (f *registrationCaptureFFI) Register(optionsJSON string) (json.RawMessage, error) {
	f.payloads = append(f.payloads, optionsJSON)
	return f.response, f.err
}

func TestInitContractExistingIdentityRegister(t *testing.T) {
	contract := loadInitContractFixture(t).ExistingIdentityRegister
	if len(contract.Cases) == 0 {
		t.Fatal("missing existing-identity registration cases")
	}
	for _, tc := range contract.Cases {
		t.Run(tc.Name, func(t *testing.T) {
			ffi := &registrationCaptureFFI{response: contract.Response}
			client := &Client{ffi: ffi}
			result, err := client.Register(context.Background(), RegisterOptions{
				AgentJSON:       tc.Request["agent_json"],
				PublicKey:       tc.Request["public_key_pem"],
				OwnerEmail:      tc.Request["owner_email"],
				RegistrationKey: tc.Request["registration_key"],
			})
			if err != nil {
				t.Fatal(err)
			}
			if len(ffi.payloads) != 1 {
				t.Fatal("expected one existing-identity FFI registration call")
			}
			var payload map[string]string
			if err := json.Unmarshal([]byte(ffi.payloads[0]), &payload); err != nil {
				t.Fatal(err)
			}
			if !reflect.DeepEqual(payload, tc.Request) {
				t.Fatal("FFI registration payload does not match shared contract")
			}
			var expected RegistrationResult
			if err := json.Unmarshal(contract.Response, &expected); err != nil {
				t.Fatal(err)
			}
			if result.AgentID != expected.AgentID {
				t.Fatal("registration response was not preserved")
			}
		})
	}
}

func loadInitContractFixture(t *testing.T) initContractFixture {
	t.Helper()

	data, err := os.ReadFile("../fixtures/init_contract.json")
	if err != nil {
		t.Fatalf("read init contract fixture: %v", err)
	}

	var fixture initContractFixture
	if err := json.Unmarshal(data, &fixture); err != nil {
		t.Fatalf("decode init contract fixture: %v", err)
	}
	return fixture
}

func TestInitContractKeyCandidateOrder(t *testing.T) {
	fixture := loadInitContractFixture(t)
	expectedDiscovery := []string{"explicit_path", "JACS_CONFIG_PATH", "./jacs.config.json"}
	if strings.Join(fixture.ConfigDiscoveryOrder, "|") != strings.Join(expectedDiscovery, "|") {
		t.Fatalf("unexpected config discovery order: got %v, want %v", fixture.ConfigDiscoveryOrder, expectedDiscovery)
	}

	expectedPasswordSources := []string{"JACS_PRIVATE_KEY_PASSWORD", "JACS_PASSWORD_FILE"}
	if strings.Join(fixture.PrivateKeyPasswordSources, "|") != strings.Join(expectedPasswordSources, "|") {
		t.Fatalf("unexpected password source list: got %v, want %v", fixture.PrivateKeyPasswordSources, expectedPasswordSources)
	}
	if fixture.PrivateKeyPasswordStrategy != "single_source_required" {
		t.Fatalf("unexpected password strategy: got %q", fixture.PrivateKeyPasswordStrategy)
	}

	tmpDir := t.TempDir()
	keyDir := filepath.Join(tmpDir, "keys")
	if err := os.MkdirAll(keyDir, 0o755); err != nil {
		t.Fatalf("mkdir key dir: %v", err)
	}

	cfg := &Config{
		JacsAgentName: "agent-alpha",
		JacsKeyDir:    "./keys",
	}
	configPath := filepath.Join(tmpDir, "jacs.config.json")

	candidateNames := make([]string, 0, len(fixture.PrivateKeyCandidateOrder))
	for _, raw := range fixture.PrivateKeyCandidateOrder {
		candidateNames = append(candidateNames, strings.ReplaceAll(raw, "{agentName}", cfg.JacsAgentName))
	}
	candidatePaths := []string{
		filepath.Join(keyDir, candidateNames[0]),
		filepath.Join(keyDir, candidateNames[1]),
		filepath.Join(keyDir, candidateNames[2]),
	}

	for i, p := range candidatePaths {
		if err := os.WriteFile(p, []byte("key-"+candidateNames[i]), 0o600); err != nil {
			t.Fatalf("write candidate key: %v", err)
		}
	}

	got := ResolveKeyPath(cfg, configPath)
	if got != candidatePaths[0] {
		t.Fatalf("expected first candidate %q, got %q", candidatePaths[0], got)
	}

	if err := os.Remove(candidatePaths[0]); err != nil {
		t.Fatalf("remove first candidate: %v", err)
	}
	got = ResolveKeyPath(cfg, configPath)
	if got != candidatePaths[1] {
		t.Fatalf("expected second candidate %q, got %q", candidatePaths[1], got)
	}

	if err := os.Remove(candidatePaths[1]); err != nil {
		t.Fatalf("remove second candidate: %v", err)
	}
	got = ResolveKeyPath(cfg, configPath)
	if got != candidatePaths[2] {
		t.Fatalf("expected third candidate %q, got %q", candidatePaths[2], got)
	}
}

func TestInitContractBootstrapRegister(t *testing.T) {
	fixture := loadInitContractFixture(t)

	// With FFI, the Go SDK delegates bootstrap registration to Rust.
	// The mock FFI client's RegisterNewAgent posts to /api/v1/agents/register
	// on the httptest server (no auth header), matching the contract.
	var gotAuth string
	var gotMethod string
	var gotPath string

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotMethod = r.Method
		gotPath = r.URL.Path
		gotAuth = r.Header.Get("Authorization")

		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{
			"agent_id":"agent-123",
			"jacs_id":"jacs-123",
			"success":true,
			"dns_verified":false,
			"name":"agent-alpha",
			"public_key_path":"/tmp/pub.pem",
			"private_key_path":"/tmp/priv.pem",
			"config_path":"/tmp/jacs.config.json",
			"registrations":[]
		}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	_, err := cl.RegisterNewAgent(
		context.Background(),
		"agent-alpha",
		&RegisterNewAgentOptions{
			OwnerEmail:  "owner@hai.ai",
			Domain:      "agent.example",
			Description: "Go init contract",
			Password:    "test-password",
			Quiet:       true,
		},
	)
	if err != nil {
		t.Fatalf("RegisterNewAgent: %v", err)
	}

	if gotMethod != fixture.BootstrapRegister.Method {
		t.Fatalf("unexpected method: got %s, want %s", gotMethod, fixture.BootstrapRegister.Method)
	}
	if gotPath != fixture.BootstrapRegister.Path {
		t.Fatalf("unexpected path: got %s, want %s", gotPath, fixture.BootstrapRegister.Path)
	}
	if fixture.BootstrapRegister.AuthRequired && gotAuth == "" {
		t.Fatal("expected auth header but none was sent")
	}
	if !fixture.BootstrapRegister.AuthRequired && gotAuth != "" {
		t.Fatalf("expected no Authorization header, got %q", gotAuth)
	}
}

func (f *registrationCaptureFFI) RegisterNewAgent(optionsJSON string) (json.RawMessage, error) {
	f.payloads = append(f.payloads, optionsJSON)
	return f.response, f.err
}

func TestInitContractRegistrationOutcomes(t *testing.T) {
	for _, tc := range loadInitContractFixture(t).RegistrationOutcomes.Cases {
		for _, bootstrap := range []bool{false, true} {
			t.Run(fmt.Sprintf("%s/bootstrap=%t", tc.Name, bootstrap), func(t *testing.T) {
				ffi := &registrationCaptureFFI{response: tc.Response}
				if tc.HTTPStatus >= 400 {
					ffi.err = fmt.Errorf("API rejection: %s", tc.Response)
				}
				client := &Client{ffi: ffi}
				var status, email *string
				var agentID string
				var err error
				if bootstrap {
					var result *RegisterResult
					result, err = client.RegisterNewAgent(context.Background(), "requested-agent", &RegisterNewAgentOptions{
						OwnerEmail: "owner@example.test", Password: "synthetic-password", Quiet: true,
					})
					if result != nil {
						status, email, agentID = result.RegistrationStatus, result.Email, result.AgentID
					}
				} else {
					var result *RegistrationResult
					result, err = client.Register(context.Background(), RegisterOptions{AgentJSON: `{"jacsId":"existing"}`})
					if result != nil {
						status, email, agentID = result.RegistrationStatus, result.Email, result.AgentID
					}
				}
				if len(ffi.payloads) != 1 {
					t.Fatal("expected exactly one FFI call")
				}
				if tc.HTTPStatus >= 400 {
					if err == nil || !strings.Contains(err.Error(), "Registration key was not accepted") {
						t.Fatalf("expected registration rejection, got %v", err)
					}
					return
				}
				if err != nil {
					t.Fatal(err)
				}
				var response struct {
					AgentID string `json:"agent_id"`
				}
				if err := json.Unmarshal(tc.Response, &response); err != nil {
					t.Fatal(err)
				}
				if agentID != response.AgentID || !reflect.DeepEqual(status, tc.ExpectedStatus) || !reflect.DeepEqual(email, tc.ExpectedEmail) {
					t.Fatalf("registration outcome not preserved: status=%v email=%v agent=%s", status, email, agentID)
				}
			})
		}
	}
}
