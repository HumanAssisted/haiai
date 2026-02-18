package main

import (
	"bufio"
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"os"
	"strconv"

	haisdk "github.com/HumanAssisted/haisdk-go"
)

// MCP JSON-RPC types (minimal stdio implementation).

type jsonRPCRequest struct {
	JSONRPC string          `json:"jsonrpc"`
	ID      json.RawMessage `json:"id,omitempty"`
	Method  string          `json:"method"`
	Params  json.RawMessage `json:"params,omitempty"`
}

type jsonRPCResponse struct {
	JSONRPC string          `json:"jsonrpc"`
	ID      json.RawMessage `json:"id,omitempty"`
	Result  interface{}     `json:"result,omitempty"`
	Error   *jsonRPCError   `json:"error,omitempty"`
}

type jsonRPCError struct {
	Code    int    `json:"code"`
	Message string `json:"message"`
}

type mcpTool struct {
	Name        string          `json:"name"`
	Description string          `json:"description"`
	InputSchema json.RawMessage `json:"inputSchema"`
}

type mcpServerInfo struct {
	Name    string `json:"name"`
	Version string `json:"version"`
}

var mcpTools = []mcpTool{
	{
		Name:        "hai_register_agent",
		Description: "Register a new JACS agent with HAI.AI",
		InputSchema: json.RawMessage(`{"type":"object","properties":{"name":{"type":"string","description":"Agent name"},"owner_email":{"type":"string","description":"Owner email"}},"required":["name","owner_email"]}`),
	},
	{
		Name:        "hai_hello",
		Description: "Test connectivity and authentication with HAI",
		InputSchema: json.RawMessage(`{"type":"object","properties":{}}`),
	},
	{
		Name:        "hai_run_benchmark",
		Description: "Run a benchmark at the specified tier",
		InputSchema: json.RawMessage(`{"type":"object","properties":{"tier":{"type":"string","description":"Benchmark tier: free, dns_certified, fully_certified","default":"free"}}}`),
	},
	{
		Name:        "hai_verify_agent",
		Description: "Verify another agent's registration status",
		InputSchema: json.RawMessage(`{"type":"object","properties":{"jacs_id":{"type":"string","description":"JACS ID of agent to verify"}},"required":["jacs_id"]}`),
	},
	{
		Name:        "hai_check_username",
		Description: "Check if a username is available for @hai.ai email",
		InputSchema: json.RawMessage(`{"type":"object","properties":{"username":{"type":"string","description":"Username to check"}},"required":["username"]}`),
	},
	{
		Name:        "hai_claim_username",
		Description: "Claim a username for an agent",
		InputSchema: json.RawMessage(`{"type":"object","properties":{"username":{"type":"string","description":"Username to claim"},"agent_id":{"type":"string","description":"Agent ID"}},"required":["username","agent_id"]}`),
	},
	{
		Name:        "hai_send_email",
		Description: "Send an email from the agent",
		InputSchema: json.RawMessage(`{"type":"object","properties":{"to":{"type":"string","description":"Recipient address"},"subject":{"type":"string","description":"Email subject"},"body":{"type":"string","description":"Email body"}},"required":["to","subject","body"]}`),
	},
	{
		Name:        "hai_list_messages",
		Description: "List agent email messages",
		InputSchema: json.RawMessage(`{"type":"object","properties":{"limit":{"type":"integer","default":20},"offset":{"type":"integer","default":0},"folder":{"type":"string","default":"inbox"}}}`),
	},
	{
		Name:        "hai_fetch_key",
		Description: "Fetch a public key from HAI's key distribution service",
		InputSchema: json.RawMessage(`{"type":"object","properties":{"agent_id":{"type":"string","description":"Agent ID"},"version":{"type":"string","default":"latest"}},"required":["agent_id"]}`),
	},
}

func cmdMCPServe(args []string) {
	fs := flag.NewFlagSet("mcp-serve", flag.ExitOnError)
	fs.Parse(args)

	scanner := bufio.NewScanner(os.Stdin)
	scanner.Buffer(make([]byte, 1024*1024), 1024*1024)

	for scanner.Scan() {
		line := scanner.Bytes()
		if len(line) == 0 {
			continue
		}

		var req jsonRPCRequest
		if err := json.Unmarshal(line, &req); err != nil {
			writeResponse(os.Stdout, jsonRPCResponse{
				JSONRPC: "2.0",
				Error:   &jsonRPCError{Code: -32700, Message: "parse error"},
			})
			continue
		}

		resp := handleMCPRequest(req)
		writeResponse(os.Stdout, resp)
	}
}

func handleMCPRequest(req jsonRPCRequest) jsonRPCResponse {
	switch req.Method {
	case "initialize":
		return jsonRPCResponse{
			JSONRPC: "2.0",
			ID:      req.ID,
			Result: map[string]interface{}{
				"protocolVersion": "2024-11-05",
				"capabilities":    map[string]interface{}{"tools": map[string]interface{}{}},
				"serverInfo":      mcpServerInfo{Name: "hai-sdk", Version: "0.1.0"},
			},
		}

	case "tools/list":
		return jsonRPCResponse{
			JSONRPC: "2.0",
			ID:      req.ID,
			Result:  map[string]interface{}{"tools": mcpTools},
		}

	case "tools/call":
		return handleToolCall(req)

	default:
		return jsonRPCResponse{
			JSONRPC: "2.0",
			ID:      req.ID,
			Error:   &jsonRPCError{Code: -32601, Message: fmt.Sprintf("method not found: %s", req.Method)},
		}
	}
}

func handleToolCall(req jsonRPCRequest) jsonRPCResponse {
	var params struct {
		Name      string          `json:"name"`
		Arguments json.RawMessage `json:"arguments"`
	}
	if err := json.Unmarshal(req.Params, &params); err != nil {
		return jsonRPCResponse{
			JSONRPC: "2.0",
			ID:      req.ID,
			Error:   &jsonRPCError{Code: -32602, Message: "invalid params"},
		}
	}

	ctx := context.Background()
	result, err := executeTool(ctx, params.Name, params.Arguments)
	if err != nil {
		return jsonRPCResponse{
			JSONRPC: "2.0",
			ID:      req.ID,
			Result: map[string]interface{}{
				"content": []map[string]interface{}{
					{"type": "text", "text": fmt.Sprintf("error: %v", err)},
				},
				"isError": true,
			},
		}
	}

	text, _ := json.Marshal(result)
	return jsonRPCResponse{
		JSONRPC: "2.0",
		ID:      req.ID,
		Result: map[string]interface{}{
			"content": []map[string]interface{}{
				{"type": "text", "text": string(text)},
			},
		},
	}
}

func executeTool(ctx context.Context, name string, argsJSON json.RawMessage) (interface{}, error) {
	cl, err := haisdk.NewClient()
	if err != nil {
		return nil, err
	}

	var args map[string]interface{}
	if len(argsJSON) > 0 {
		json.Unmarshal(argsJSON, &args)
	}

	getString := func(key string) string {
		v, _ := args[key].(string)
		return v
	}
	getInt := func(key string, def int) int {
		if v, ok := args[key].(float64); ok {
			return int(v)
		}
		if v, ok := args[key].(string); ok {
			if n, err := strconv.Atoi(v); err == nil {
				return n
			}
		}
		return def
	}

	switch name {
	case "hai_register_agent":
		return cl.RegisterNewAgent(ctx, getString("name"), &haisdk.RegisterNewAgentOptions{
			OwnerEmail: getString("owner_email"),
		})

	case "hai_hello":
		return cl.Hello(ctx)

	case "hai_run_benchmark":
		tier := getString("tier")
		if tier == "" {
			tier = "free"
		}
		return cl.Benchmark(ctx, tier)

	case "hai_verify_agent":
		return cl.VerifyAgent(ctx, getString("jacs_id"))

	case "hai_check_username":
		return cl.CheckUsername(ctx, getString("username"))

	case "hai_claim_username":
		return cl.ClaimUsername(ctx, getString("agent_id"), getString("username"))

	case "hai_send_email":
		return cl.SendEmail(ctx, getString("to"), getString("subject"), getString("body"))

	case "hai_list_messages":
		folder := getString("folder")
		if folder == "" {
			folder = "inbox"
		}
		return cl.ListMessages(ctx, haisdk.ListMessagesOptions{
			Limit:  getInt("limit", 20),
			Offset: getInt("offset", 0),
			Folder: folder,
		})

	case "hai_fetch_key":
		version := getString("version")
		if version == "" {
			version = "latest"
		}
		return cl.FetchRemoteKey(ctx, getString("agent_id"), version)

	default:
		return nil, fmt.Errorf("unknown tool: %s", name)
	}
}

func writeResponse(w io.Writer, resp jsonRPCResponse) {
	data, _ := json.Marshal(resp)
	fmt.Fprintf(w, "%s\n", data)
}
