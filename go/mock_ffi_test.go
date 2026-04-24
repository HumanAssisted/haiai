package haiai

import (
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strings"
)

// mockFFIClient implements FFIClient by delegating to an httptest.Server.
// This bridges the old httptest-based tests with the new FFI-based client.
type mockFFIClient struct {
	// httpHandler is called for each FFI method; the mock routes the method
	// name and args to the underlying HTTP handler.
	baseURL    string
	httpClient *http.Client
	jacsID     string
	agentID    string
	agentEmail string
	authHeader string

	// buildAuthHeaderFn returns the auth header. Tests can override this.
	buildAuthHeaderFn func() (string, error)
	// signMessageFn signs a message. Tests can override this.
	signMessageFn func(message string) (string, error)
	// exportAgentJSONFn overrides ExportAgentJSON. Tests can override this.
	exportAgentJSONFn func() (json.RawMessage, error)
	// verifyA2AArtifactFn overrides VerifyA2AArtifact. Tests can override this.
	verifyA2AArtifactFn func(wrappedJSON string) (json.RawMessage, error)
	// rotateKeysFn overrides RotateKeys for unit tests. If nil, falls back to doPost.
	rotateKeysFn func(optionsJSON string) (json.RawMessage, error)
}

func newMockFFIClient(baseURL, jacsID, authHeader string) *mockFFIClient {
	return &mockFFIClient{
		baseURL:    strings.TrimRight(baseURL, "/"),
		httpClient: &http.Client{},
		jacsID:     jacsID,
		agentID:    jacsID,
		authHeader: authHeader,
	}
}

func (m *mockFFIClient) Close() {}

func (m *mockFFIClient) doGet(path string) (json.RawMessage, error) {
	req, err := http.NewRequest(http.MethodGet, m.baseURL+path, nil)
	if err != nil {
		return nil, err
	}
	if m.authHeader != "" {
		req.Header.Set("Authorization", m.authHeader)
	}
	return m.doHTTP(req)
}

func (m *mockFFIClient) doPost(path string, body interface{}) (json.RawMessage, error) {
	var bodyReader io.Reader
	if body != nil {
		data, err := json.Marshal(body)
		if err != nil {
			return nil, err
		}
		bodyReader = strings.NewReader(string(data))
	}
	req, err := http.NewRequest(http.MethodPost, m.baseURL+path, bodyReader)
	if err != nil {
		return nil, err
	}
	req.Header.Set("Content-Type", "application/json")
	if m.authHeader != "" {
		req.Header.Set("Authorization", m.authHeader)
	}
	return m.doHTTP(req)
}

func (m *mockFFIClient) doPut(path string, body interface{}) (json.RawMessage, error) {
	var bodyReader io.Reader
	if body != nil {
		data, err := json.Marshal(body)
		if err != nil {
			return nil, err
		}
		bodyReader = strings.NewReader(string(data))
	}
	req, err := http.NewRequest(http.MethodPut, m.baseURL+path, bodyReader)
	if err != nil {
		return nil, err
	}
	req.Header.Set("Content-Type", "application/json")
	if m.authHeader != "" {
		req.Header.Set("Authorization", m.authHeader)
	}
	return m.doHTTP(req)
}

func (m *mockFFIClient) doDelete(path string) (json.RawMessage, error) {
	req, err := http.NewRequest(http.MethodDelete, m.baseURL+path, nil)
	if err != nil {
		return nil, err
	}
	if m.authHeader != "" {
		req.Header.Set("Authorization", m.authHeader)
	}
	return m.doHTTP(req)
}

func (m *mockFFIClient) doHTTP(req *http.Request) (json.RawMessage, error) {
	resp, err := m.httpClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()
	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, err
	}
	if resp.StatusCode == http.StatusNoContent {
		return json.RawMessage("null"), nil
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, fmt.Errorf("HTTP %d: %s", resp.StatusCode, string(body))
	}
	return json.RawMessage(body), nil
}

// --- Registration & Identity ---

func (m *mockFFIClient) Hello(includeTest bool) (json.RawMessage, error) {
	body := map[string]interface{}{
		"agent_id": m.jacsID,
	}
	if includeTest {
		body["include_test"] = true
	}
	return m.doPost("/api/v1/agents/hello", body)
}

func (m *mockFFIClient) doGetNoAuth(path string) (json.RawMessage, error) {
	req, err := http.NewRequest(http.MethodGet, m.baseURL+path, nil)
	if err != nil {
		return nil, err
	}
	// No auth header for public endpoints
	return m.doHTTP(req)
}

func (m *mockFFIClient) doPostNoAuth(path string, body interface{}) (json.RawMessage, error) {
	var bodyReader io.Reader
	if body != nil {
		data, err := json.Marshal(body)
		if err != nil {
			return nil, err
		}
		bodyReader = strings.NewReader(string(data))
	}
	req, err := http.NewRequest(http.MethodPost, m.baseURL+path, bodyReader)
	if err != nil {
		return nil, err
	}
	req.Header.Set("Content-Type", "application/json")
	// No auth header for public endpoints
	return m.doHTTP(req)
}

func (m *mockFFIClient) Register(optionsJSON string) (json.RawMessage, error) {
	return m.doPost("/api/v1/agents/register", json.RawMessage(optionsJSON))
}

func (m *mockFFIClient) RegisterNewAgent(optionsJSON string) (json.RawMessage, error) {
	return m.doPostNoAuth("/api/v1/agents/register", json.RawMessage(optionsJSON))
}

func (m *mockFFIClient) RotateKeys(optionsJSON string) (json.RawMessage, error) {
	if m.rotateKeysFn != nil {
		return m.rotateKeysFn(optionsJSON)
	}
	return m.doPost("/api/v1/agents/rotate-keys", json.RawMessage(optionsJSON))
}

func (m *mockFFIClient) UpdateAgent(agentData string) (json.RawMessage, error) {
	return m.doPost("/api/v1/agents/update", json.RawMessage(agentData))
}

func (m *mockFFIClient) SubmitResponse(paramsJSON string) (json.RawMessage, error) {
	var params map[string]interface{}
	if err := json.Unmarshal([]byte(paramsJSON), &params); err != nil {
		return nil, err
	}
	jobID, _ := params["job_id"].(string)
	path := fmt.Sprintf("/api/v1/agents/jobs/%s/response", urlEncode(jobID))
	return m.doPost(path, json.RawMessage(paramsJSON))
}

func (m *mockFFIClient) VerifyStatus(agentID string) (json.RawMessage, error) {
	path := fmt.Sprintf("/api/v1/agents/%s/verify", urlEncode(agentID))
	return m.doGet(path)
}

// --- Username ---

func (m *mockFFIClient) UpdateUsername(agentID, username string) (json.RawMessage, error) {
	path := fmt.Sprintf("/api/v1/agents/%s/username", urlEncode(agentID))
	return m.doPut(path, map[string]string{"username": username})
}

func (m *mockFFIClient) DeleteUsername(agentID string) (json.RawMessage, error) {
	path := fmt.Sprintf("/api/v1/agents/%s/username", urlEncode(agentID))
	return m.doDelete(path)
}

// --- Email Core ---

func (m *mockFFIClient) SendEmail(optionsJSON string) (json.RawMessage, error) {
	path := fmt.Sprintf("/api/agents/%s/email/send", urlEncode(m.agentID))
	raw, err := m.doPostWithEmailErrors(path, json.RawMessage(optionsJSON))
	return raw, err
}

// doPostWithEmailErrors is like doPost but maps email-specific HTTP errors to sentinel errors.
func (m *mockFFIClient) doPostWithEmailErrors(path string, body interface{}) (json.RawMessage, error) {
	var bodyReader io.Reader
	if body != nil {
		data, err := json.Marshal(body)
		if err != nil {
			return nil, err
		}
		bodyReader = strings.NewReader(string(data))
	}
	req, err := http.NewRequest(http.MethodPost, m.baseURL+path, bodyReader)
	if err != nil {
		return nil, err
	}
	req.Header.Set("Content-Type", "application/json")
	if m.authHeader != "" {
		req.Header.Set("Authorization", m.authHeader)
	}
	resp, err := m.httpClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()
	respBody, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, err
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, classifyEmailError(resp.StatusCode, respBody)
	}
	return json.RawMessage(respBody), nil
}

func (m *mockFFIClient) SendSignedEmail(optionsJSON string) (json.RawMessage, error) {
	return m.SendEmail(optionsJSON)
}

func (m *mockFFIClient) ListMessages(optionsJSON string) (json.RawMessage, error) {
	var opts ListMessagesOptions
	_ = json.Unmarshal([]byte(optionsJSON), &opts)
	query := buildListMessagesQuery(opts)
	path := fmt.Sprintf("/api/agents/%s/email/messages?%s", urlEncode(m.agentID), query)
	return m.doGet(path)
}

func (m *mockFFIClient) UpdateLabels(paramsJSON string) (json.RawMessage, error) {
	path := fmt.Sprintf("/api/agents/%s/email/labels", urlEncode(m.agentID))
	return m.doPost(path, json.RawMessage(paramsJSON))
}

func (m *mockFFIClient) GetEmailStatus() (json.RawMessage, error) {
	path := fmt.Sprintf("/api/agents/%s/email/status", urlEncode(m.agentID))
	return m.doGet(path)
}

func (m *mockFFIClient) GetMessage(messageID string) (json.RawMessage, error) {
	path := fmt.Sprintf("/api/agents/%s/email/messages/%s", urlEncode(m.agentID), urlEncode(messageID))
	return m.doGet(path)
}

func (m *mockFFIClient) GetRawEmail(messageID string) (json.RawMessage, error) {
	path := fmt.Sprintf("/api/agents/%s/email/messages/%s/raw", urlEncode(m.agentID), urlEncode(messageID))
	return m.doGet(path)
}

func (m *mockFFIClient) GetUnreadCount() (json.RawMessage, error) {
	path := fmt.Sprintf("/api/agents/%s/email/unread-count", urlEncode(m.agentID))
	return m.doGet(path)
}

// --- Email Actions ---

func (m *mockFFIClient) MarkRead(messageID string) error {
	path := fmt.Sprintf("/api/agents/%s/email/messages/%s/read", urlEncode(m.agentID), urlEncode(messageID))
	_, err := m.doPost(path, nil)
	return err
}

func (m *mockFFIClient) MarkUnread(messageID string) error {
	path := fmt.Sprintf("/api/agents/%s/email/messages/%s/unread", urlEncode(m.agentID), urlEncode(messageID))
	_, err := m.doPost(path, nil)
	return err
}

func (m *mockFFIClient) DeleteMessage(messageID string) error {
	path := fmt.Sprintf("/api/agents/%s/email/messages/%s", urlEncode(m.agentID), urlEncode(messageID))
	_, err := m.doDelete(path)
	return err
}

func (m *mockFFIClient) Archive(messageID string) error {
	path := fmt.Sprintf("/api/agents/%s/email/messages/%s/archive", urlEncode(m.agentID), urlEncode(messageID))
	_, err := m.doPost(path, nil)
	return err
}

func (m *mockFFIClient) Unarchive(messageID string) error {
	path := fmt.Sprintf("/api/agents/%s/email/messages/%s/unarchive", urlEncode(m.agentID), urlEncode(messageID))
	_, err := m.doPost(path, nil)
	return err
}

func (m *mockFFIClient) ReplyWithOptions(paramsJSON string) (json.RawMessage, error) {
	path := fmt.Sprintf("/api/agents/%s/email/reply", urlEncode(m.agentID))
	return m.doPost(path, json.RawMessage(paramsJSON))
}

func (m *mockFFIClient) Forward(paramsJSON string) (json.RawMessage, error) {
	path := fmt.Sprintf("/api/agents/%s/email/forward", urlEncode(m.agentID))
	return m.doPost(path, json.RawMessage(paramsJSON))
}

// --- Search & Contacts ---

func (m *mockFFIClient) SearchMessages(optionsJSON string) (json.RawMessage, error) {
	var opts SearchOptions
	_ = json.Unmarshal([]byte(optionsJSON), &opts)
	query := buildSearchQuery(opts)
	path := fmt.Sprintf("/api/agents/%s/email/search?%s", urlEncode(m.agentID), query)
	return m.doGet(path)
}

func (m *mockFFIClient) Contacts() (json.RawMessage, error) {
	path := fmt.Sprintf("/api/agents/%s/email/contacts", urlEncode(m.agentID))
	return m.doGet(path)
}

// --- Key Operations ---

func (m *mockFFIClient) FetchRemoteKey(jacsID, version string) (json.RawMessage, error) {
	path := fmt.Sprintf("/api/agents/keys/%s/%s", urlEncode(jacsID), urlEncode(version))
	return m.doGetNoAuth(path)
}

func (m *mockFFIClient) FetchKeyByHash(hash string) (json.RawMessage, error) {
	path := fmt.Sprintf("/api/agents/keys/hash/%s", urlEncode(hash))
	return m.doGetNoAuth(path)
}

func (m *mockFFIClient) FetchKeyByEmail(email string) (json.RawMessage, error) {
	path := fmt.Sprintf("/api/agents/keys/%s", urlEncode(email))
	return m.doGetNoAuth(path)
}

func (m *mockFFIClient) FetchKeyByDomain(domain string) (json.RawMessage, error) {
	path := fmt.Sprintf("/api/agents/keys/domain/%s", urlEncode(domain))
	return m.doGetNoAuth(path)
}

func (m *mockFFIClient) FetchAllKeys(jacsID string) (json.RawMessage, error) {
	path := fmt.Sprintf("/api/agents/keys/%s/all", urlEncode(jacsID))
	return m.doGetNoAuth(path)
}

// --- Verification ---

func (m *mockFFIClient) VerifyDocument(document string) (json.RawMessage, error) {
	body := map[string]string{"document": document}
	return m.doPostNoAuth("/api/jacs/verify", body)
}

func (m *mockFFIClient) GetVerification(agentID string) (json.RawMessage, error) {
	path := fmt.Sprintf("/api/v1/agents/%s/verification", urlEncode(agentID))
	return m.doGetNoAuth(path)
}

func (m *mockFFIClient) VerifyAgentDocument(requestJSON string) (json.RawMessage, error) {
	return m.doPostNoAuth("/api/v1/agents/verify", json.RawMessage(requestJSON))
}

// --- Benchmarks ---

func (m *mockFFIClient) Benchmark(name, tier string) (json.RawMessage, error) {
	body := map[string]string{
		"name":      name,
		"tier":      tier,
		"transport": "sse",
	}
	return m.doPost("/api/benchmark/run", body)
}

func (m *mockFFIClient) FreeRun(transport string) (json.RawMessage, error) {
	return m.Benchmark("", "free")
}

func (m *mockFFIClient) ProRun(optionsJSON string) (json.RawMessage, error) {
	// For the mock, delegate to the full ProRun flow via HTTP
	// First purchase
	purchaseResp, err := m.doPost("/api/benchmark/purchase", map[string]string{"tier": "pro"})
	if err != nil {
		return nil, err
	}
	var sub struct {
		CheckoutURL string `json:"checkout_url"`
		SessionID   string `json:"session_id"`
		AlreadyPaid bool   `json:"already_paid"`
	}
	if err := json.Unmarshal(purchaseResp, &sub); err != nil {
		return nil, err
	}
	if !sub.AlreadyPaid && sub.CheckoutURL != "" {
		// Poll for payment
		statusPath := fmt.Sprintf("/api/benchmark/payments/%s/status", urlEncode(sub.SessionID))
		_, _ = m.doGet(statusPath)
	}
	// Run benchmark
	return m.Benchmark("", "pro")
}

func (m *mockFFIClient) EnterpriseRun() error {
	return fmt.Errorf("the enterprise tier is coming soon; contact support@hai.ai for early access")
}

// --- JACS Delegation ---

func (m *mockFFIClient) BuildAuthHeader() (string, error) {
	if m.buildAuthHeaderFn != nil {
		return m.buildAuthHeaderFn()
	}
	return m.authHeader, nil
}

func (m *mockFFIClient) SignMessage(message string) (string, error) {
	if m.signMessageFn != nil {
		return m.signMessageFn(message)
	}
	return "", fmt.Errorf("mock: SignMessage not implemented")
}

func (m *mockFFIClient) CanonicalJSON(valueJSON string) (string, error) {
	return valueJSON, nil
}

func (m *mockFFIClient) VerifyA2AArtifact(wrappedJSON string) (json.RawMessage, error) {
	if m.verifyA2AArtifactFn != nil {
		return m.verifyA2AArtifactFn(wrappedJSON)
	}
	return nil, fmt.Errorf("mock: VerifyA2AArtifact not implemented")
}

func (m *mockFFIClient) ExportAgentJSON() (json.RawMessage, error) {
	if m.exportAgentJSONFn != nil {
		return m.exportAgentJSONFn()
	}
	return nil, fmt.Errorf("mock: ExportAgentJSON not implemented")
}

// --- Client State ---

func (m *mockFFIClient) JacsID() (string, error) {
	return m.jacsID, nil
}

func (m *mockFFIClient) BaseURL() (string, error) {
	return m.baseURL, nil
}

func (m *mockFFIClient) HaiAgentID() (string, error) {
	return m.agentID, nil
}

func (m *mockFFIClient) AgentEmail() (string, error) {
	return m.agentEmail, nil
}

func (m *mockFFIClient) SetHaiAgentID(id string) error {
	m.agentID = id
	return nil
}

func (m *mockFFIClient) SetAgentEmail(email string) error {
	m.agentEmail = email
	return nil
}

// --- Server Keys ---

func (m *mockFFIClient) FetchServerKeys() (json.RawMessage, error) {
	return m.doGet("/api/v1/keys/server")
}

// --- Email Sign/Verify (raw, base64-encoded) ---

func (m *mockFFIClient) SignEmailRaw(rawEmailB64 string) (json.RawMessage, error) {
	// Decode base64 -> raw email, send as message/rfc822, return base64-encoded result as JSON string
	decoded, err := base64.StdEncoding.DecodeString(rawEmailB64)
	if err != nil {
		return nil, err
	}
	req, err := http.NewRequest(http.MethodPost, m.baseURL+"/api/v1/email/sign", strings.NewReader(string(decoded)))
	if err != nil {
		return nil, err
	}
	req.Header.Set("Content-Type", "message/rfc822")
	if m.authHeader != "" {
		req.Header.Set("Authorization", m.authHeader)
	}
	resp, err := m.httpClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()
	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, err
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, fmt.Errorf("HTTP %d: %s", resp.StatusCode, string(body))
	}
	// Return the response body as a JSON-encoded base64 string
	b64Result := base64.StdEncoding.EncodeToString(body)
	resultJSON, _ := json.Marshal(b64Result)
	return json.RawMessage(resultJSON), nil
}

func (m *mockFFIClient) VerifyEmailRaw(rawEmailB64 string) (json.RawMessage, error) {
	// Decode base64 -> raw email, send as message/rfc822, return JSON response
	decoded, err := base64.StdEncoding.DecodeString(rawEmailB64)
	if err != nil {
		return nil, err
	}
	req, err := http.NewRequest(http.MethodPost, m.baseURL+"/api/v1/email/verify", strings.NewReader(string(decoded)))
	if err != nil {
		return nil, err
	}
	req.Header.Set("Content-Type", "message/rfc822")
	if m.authHeader != "" {
		req.Header.Set("Authorization", m.authHeader)
	}
	return m.doHTTP(req)
}

// --- Attestations ---

func (m *mockFFIClient) CreateAttestation(paramsJSON string) (json.RawMessage, error) {
	return m.doPost("/api/v1/attestations", json.RawMessage(paramsJSON))
}

func (m *mockFFIClient) ListAttestations(paramsJSON string) (json.RawMessage, error) {
	var params struct {
		AgentID string `json:"agent_id"`
		Limit   int    `json:"limit"`
		Offset  int    `json:"offset"`
	}
	_ = json.Unmarshal([]byte(paramsJSON), &params)
	path := fmt.Sprintf("/api/v1/agents/%s/attestations?limit=%d&offset=%d", urlEncode(params.AgentID), params.Limit, params.Offset)
	return m.doGet(path)
}

func (m *mockFFIClient) GetAttestation(agentID, docID string) (json.RawMessage, error) {
	path := fmt.Sprintf("/api/v1/agents/%s/attestations/%s", urlEncode(agentID), urlEncode(docID))
	return m.doGet(path)
}

func (m *mockFFIClient) VerifyAttestation(document string) (json.RawMessage, error) {
	return m.doPostNoAuth("/api/v1/attestations/verify", map[string]string{"document": document})
}

// --- Email Templates ---

func (m *mockFFIClient) CreateEmailTemplate(optionsJSON string) (json.RawMessage, error) {
	path := fmt.Sprintf("/api/agents/%s/email/templates", urlEncode(m.agentID))
	return m.doPost(path, json.RawMessage(optionsJSON))
}

func (m *mockFFIClient) ListEmailTemplates(optionsJSON string) (json.RawMessage, error) {
	path := fmt.Sprintf("/api/agents/%s/email/templates", urlEncode(m.agentID))
	return m.doGet(path)
}

func (m *mockFFIClient) GetEmailTemplate(templateID string) (json.RawMessage, error) {
	path := fmt.Sprintf("/api/agents/%s/email/templates/%s", urlEncode(m.agentID), urlEncode(templateID))
	return m.doGet(path)
}

func (m *mockFFIClient) UpdateEmailTemplate(templateID, optionsJSON string) (json.RawMessage, error) {
	path := fmt.Sprintf("/api/agents/%s/email/templates/%s", urlEncode(m.agentID), urlEncode(templateID))
	return m.doPut(path, json.RawMessage(optionsJSON))
}

func (m *mockFFIClient) DeleteEmailTemplate(templateID string) (json.RawMessage, error) {
	path := fmt.Sprintf("/api/agents/%s/email/templates/%s", urlEncode(m.agentID), urlEncode(templateID))
	return m.doDelete(path)
}

// --- SSE Streaming ---

func (m *mockFFIClient) ConnectSSE() (uint64, error) {
	return 0, fmt.Errorf("mock: ConnectSSE not implemented")
}

func (m *mockFFIClient) SSENextEvent(handleID uint64) (json.RawMessage, error) {
	return nil, fmt.Errorf("mock: SSENextEvent not implemented")
}

func (m *mockFFIClient) SSEClose(handleID uint64) {}

// --- WebSocket Streaming ---

func (m *mockFFIClient) ConnectWS() (uint64, error) {
	return 0, fmt.Errorf("mock: ConnectWS not implemented")
}

func (m *mockFFIClient) WSNextEvent(handleID uint64) (json.RawMessage, error) {
	return nil, fmt.Errorf("mock: WSNextEvent not implemented")
}

func (m *mockFFIClient) WSClose(handleID uint64) {}

// --- Helpers ---

func urlEncode(s string) string {
	// Use net/url.PathEscape for path segments
	return (&netUrlPathEscaper{}).escape(s)
}

type netUrlPathEscaper struct{}

func (e *netUrlPathEscaper) escape(s string) string {
	// Simple path escaping: replace / with %2F, etc.
	var result strings.Builder
	for _, b := range []byte(s) {
		switch {
		case b >= 'a' && b <= 'z', b >= 'A' && b <= 'Z', b >= '0' && b <= '9',
			b == '-', b == '_', b == '.', b == '~',
			b == ':', b == '@', b == '!', b == '$', b == '&', b == '\'',
			b == '(', b == ')', b == '*', b == '+', b == ',', b == ';', b == '=':
			result.WriteByte(b)
		default:
			fmt.Fprintf(&result, "%%%02X", b)
		}
	}
	return result.String()
}

func buildListMessagesQuery(opts ListMessagesOptions) string {
	q := fmt.Sprintf("limit=%d&offset=%d", opts.Limit, opts.Offset)
	if opts.Direction != "" {
		q += "&direction=" + opts.Direction
	}
	if opts.IsRead != nil {
		q += fmt.Sprintf("&is_read=%t", *opts.IsRead)
	}
	if opts.Folder != "" {
		q += "&folder=" + opts.Folder
	}
	if opts.Label != "" {
		q += "&label=" + opts.Label
	}
	if opts.HasAttachments != nil {
		q += fmt.Sprintf("&has_attachments=%t", *opts.HasAttachments)
	}
	if opts.Since != "" {
		q += "&since=" + opts.Since
	}
	if opts.Until != "" {
		q += "&until=" + opts.Until
	}
	return q
}

func buildSearchQuery(opts SearchOptions) string {
	var parts []string
	if opts.Q != "" {
		parts = append(parts, "q="+opts.Q)
	}
	if opts.Direction != "" {
		parts = append(parts, "direction="+opts.Direction)
	}
	if opts.FromAddress != "" {
		parts = append(parts, "from_address="+opts.FromAddress)
	}
	if opts.ToAddress != "" {
		parts = append(parts, "to_address="+opts.ToAddress)
	}
	if opts.Limit > 0 {
		parts = append(parts, fmt.Sprintf("limit=%d", opts.Limit))
	}
	if opts.Offset > 0 {
		parts = append(parts, fmt.Sprintf("offset=%d", opts.Offset))
	}
	if opts.IsRead != nil {
		parts = append(parts, fmt.Sprintf("is_read=%t", *opts.IsRead))
	}
	if opts.JacsVerified != nil {
		parts = append(parts, fmt.Sprintf("jacs_verified=%t", *opts.JacsVerified))
	}
	if opts.Folder != "" {
		parts = append(parts, "folder="+opts.Folder)
	}
	if opts.Label != "" {
		parts = append(parts, "label="+opts.Label)
	}
	if opts.HasAttachments != nil {
		parts = append(parts, fmt.Sprintf("has_attachments=%t", *opts.HasAttachments))
	}
	if opts.Since != "" {
		parts = append(parts, "since="+opts.Since)
	}
	if opts.Until != "" {
		parts = append(parts, "until="+opts.Until)
	}
	return strings.Join(parts, "&")
}
