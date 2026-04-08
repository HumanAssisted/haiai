package haiai

// MCP tool contract parity tests -- verify the Go FFI adapter can
// support every tool category defined in the shared MCP tool contract.
//
// The Rust MCP server (hai-mcp) is the canonical MCP implementation.
// Python, Node, and Go do not have their own MCP servers (deleted per
// CLI_PARITY_AUDIT.md). However, the FFI adapter must expose the
// underlying methods needed to serve each MCP tool category, so that
// any future MCP reimplementation is backed by the same FFI layer.
//
// This test loads fixtures/mcp_tool_contract.json and validates that:
// 1. The fixture is structurally valid (has required fields, counts match).
// 2. Every MCP tool category maps to at least one FFI adapter method.
// 3. The total_tool_count field matches the actual number of tools.

import (
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"sort"
	"testing"
)

// ---------------------------------------------------------------------------
// Fixture types
// ---------------------------------------------------------------------------

type mcpToolContract struct {
	Description    string    `json:"description"`
	Version        string    `json:"version"`
	TotalToolCount int       `json:"total_tool_count"`
	RequiredTools  []mcpTool `json:"required_tools"`
}

type mcpTool struct {
	Name       string            `json:"name"`
	Properties map[string]string `json:"properties"`
	Required   []string          `json:"required"`
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

func loadMCPContract(t *testing.T) *mcpToolContract {
	t.Helper()
	_, filename, _, _ := runtime.Caller(0)
	fixturesDir := filepath.Join(filepath.Dir(filename), "..", "fixtures")
	data, err := os.ReadFile(filepath.Join(fixturesDir, "mcp_tool_contract.json"))
	if err != nil {
		t.Fatalf("Failed to read MCP tool contract fixture: %v", err)
	}
	var contract mcpToolContract
	if err := json.Unmarshal(data, &contract); err != nil {
		t.Fatalf("Failed to parse MCP tool contract fixture: %v", err)
	}
	return &contract
}

// mcpToolToFFIMethods maps each MCP tool name to the Go FFIClient interface
// methods (PascalCase) that would back it. Tools that are client-side only
// (e.g. hai_generate_verify_link, hai_self_knowledge) map to an empty slice.
var mcpToolToFFIMethods = map[string][]string{
	"hai_hello":                  {"Hello"},
	"hai_register_agent":         {"Register"},
	"hai_agent_status":           {"VerifyStatus"},
	"hai_verify_status":          {"VerifyStatus"},
	"hai_generate_verify_link":   {}, // client-side only, no FFI method needed
	"hai_send_email":             {"SendEmail"},
	"hai_list_messages":          {"ListMessages"},
	"hai_get_message":            {"GetMessage"},
	"hai_delete_message":         {"DeleteMessage"},
	"hai_mark_read":              {"MarkRead"},
	"hai_mark_unread":            {"MarkUnread"},
	"hai_search_messages":        {"SearchMessages"},
	"hai_get_unread_count":       {"GetUnreadCount"},
	"hai_get_email_status":       {"GetEmailStatus"},
	"hai_reply_email":            {"ReplyWithOptions"},
	"hai_forward_email":          {"Forward"},
	"hai_archive_message":        {"Archive"},
	"hai_unarchive_message":      {"Unarchive"},
	"hai_list_contacts":          {"Contacts"},
	"hai_self_knowledge":         {}, // embedded docs, no FFI method needed
	"hai_create_email_template":  {"CreateEmailTemplate"},
	"hai_list_email_templates":   {"ListEmailTemplates"},
	"hai_search_email_templates": {"ListEmailTemplates"},
	"hai_get_email_template":     {"GetEmailTemplate"},
	"hai_update_email_template":  {"UpdateEmailTemplate"},
	"hai_delete_email_template":  {"DeleteEmailTemplate"},
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

func TestMCPContractFixtureHasRequiredFields(t *testing.T) {
	contract := loadMCPContract(t)
	if contract.Version == "" {
		t.Error("MCP tool contract fixture missing 'version'")
	}
	if contract.TotalToolCount == 0 {
		t.Error("MCP tool contract fixture missing or zero 'total_tool_count'")
	}
	if len(contract.RequiredTools) == 0 {
		t.Error("MCP tool contract fixture missing or empty 'required_tools'")
	}
}

func TestMCPContractTotalToolCountMatchesActual(t *testing.T) {
	contract := loadMCPContract(t)
	if contract.TotalToolCount != len(contract.RequiredTools) {
		t.Errorf("total_tool_count (%d) != len(required_tools) (%d)",
			contract.TotalToolCount, len(contract.RequiredTools))
	}
}

func TestMCPContractEveryToolHasNamePropertiesRequired(t *testing.T) {
	contract := loadMCPContract(t)
	for i, tool := range contract.RequiredTools {
		if tool.Name == "" {
			t.Errorf("Tool at index %d has no 'name'", i)
		}
		if tool.Properties == nil {
			t.Errorf("Tool %q has nil 'properties'", tool.Name)
		}
		if tool.Required == nil {
			t.Errorf("Tool %q has nil 'required'", tool.Name)
		}
	}
}

func TestFFIClientCoversAllMCPToolCategories(t *testing.T) {
	contract := loadMCPContract(t)

	// Get methods from FFIClient interface via reflection.
	ffiType := reflect.TypeOf((*FFIClient)(nil)).Elem()
	ifaceMethods := make(map[string]bool)
	for i := 0; i < ffiType.NumMethod(); i++ {
		ifaceMethods[ffiType.Method(i).Name] = true
	}

	var missing []string
	for _, tool := range contract.RequiredTools {
		ffiMethods, ok := mcpToolToFFIMethods[tool.Name]
		if !ok {
			missing = append(missing, tool.Name+": no mapping in mcpToolToFFIMethods")
			continue
		}
		for _, method := range ffiMethods {
			if !ifaceMethods[method] {
				missing = append(missing, tool.Name+" -> FFI method '"+method+"' not in FFIClient")
			}
		}
	}

	if len(missing) > 0 {
		sort.Strings(missing)
		t.Errorf("MCP tools missing FFI adapter backing:\n")
		for _, m := range missing {
			t.Errorf("  - %s", m)
		}
	}
}

func TestMCPToolMappingCoversAllFixtureTools(t *testing.T) {
	contract := loadMCPContract(t)

	fixtureNames := make(map[string]bool)
	for _, tool := range contract.RequiredTools {
		fixtureNames[tool.Name] = true
	}

	mappingNames := make(map[string]bool)
	for name := range mcpToolToFFIMethods {
		mappingNames[name] = true
	}

	var unmapped []string
	for name := range fixtureNames {
		if !mappingNames[name] {
			unmapped = append(unmapped, name)
		}
	}

	if len(unmapped) > 0 {
		sort.Strings(unmapped)
		t.Errorf("Fixture tools without mcpToolToFFIMethods mapping: %v", unmapped)
	}
}
