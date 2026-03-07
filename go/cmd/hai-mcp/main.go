// hai-mcp is an MCP server that exposes HAI SDK operations as tools.
//
// It provides identity tools (hello, register, check/claim username, verify)
// and email tools (send, list, get, delete, mark read/unread, search, reply).
//
// Usage:
//
//	hai-mcp                        # stdio transport, auto-discover jacs.config.json
//	HAI_URL=https://hai.ai hai-mcp # override API endpoint
package main

import (
	"context"
	"encoding/json"
	"fmt"
	"os"

	haisdk "github.com/HumanAssisted/haisdk-go"
	"github.com/mark3labs/mcp-go/mcp"
	"github.com/mark3labs/mcp-go/server"
)

func main() {
	s := server.NewMCPServer(
		"hai-sdk",
		"0.1.0",
		server.WithToolCapabilities(false),
	)

	s.AddTools(requiredToolDefinitions()...)

	// Backward-compatible extra tool not included in the shared minimum contract.
	s.AddTool(mcp.NewTool("hai_verify_agent",
		mcp.WithDescription("Verify another agent's identity"),
		mcp.WithString("agent_id", mcp.Required(), mcp.Description("Agent ID to verify")),
		mcp.WithString("config_path", mcp.Description("Path to jacs.config.json")),
		mcp.WithString("hai_url", mcp.Description("HAI API URL override")),
	), handleVerifyAgent)

	if err := server.ServeStdio(s); err != nil {
		fmt.Fprintf(os.Stderr, "server error: %v\n", err)
		os.Exit(1)
	}
}

func requiredToolDefinitions() []server.ServerTool {
	return []server.ServerTool{
		{
			Tool: mcp.NewTool("hai_hello",
				mcp.WithDescription("Run authenticated hello handshake with HAI"),
				mcp.WithString("config_path", mcp.Description("Path to jacs.config.json")),
				mcp.WithString("hai_url", mcp.Description("HAI API URL override")),
			),
			Handler: handleHello,
		},
		{
			Tool: mcp.NewTool("hai_check_username",
				mcp.WithDescription("Check if a hai.ai username is available"),
				mcp.WithString("username", mcp.Required(), mcp.Description("Username to check")),
				mcp.WithString("hai_url", mcp.Description("HAI API URL override")),
			),
			Handler: handleCheckUsername,
		},
		{
			Tool: mcp.NewTool("hai_claim_username",
				mcp.WithDescription("Claim a hai.ai username for an agent"),
				mcp.WithString("agent_id", mcp.Required(), mcp.Description("Agent UUID")),
				mcp.WithString("username", mcp.Required(), mcp.Description("Username to claim")),
				mcp.WithString("config_path", mcp.Description("Path to jacs.config.json")),
				mcp.WithString("hai_url", mcp.Description("HAI API URL override")),
			),
			Handler: handleClaimUsername,
		},
		{
			Tool: mcp.NewTool("hai_register_agent",
				mcp.WithDescription("Register the local JACS agent with HAI"),
				mcp.WithString("config_path", mcp.Description("Path to jacs.config.json")),
				mcp.WithString("owner_email", mcp.Description("Owner email address")),
				mcp.WithString("hai_url", mcp.Description("HAI API URL override")),
			),
			Handler: handleRegisterAgent,
		},
		{
			Tool: mcp.NewTool("hai_agent_status",
				mcp.WithDescription("Get the current agent's verification status"),
				mcp.WithString("config_path", mcp.Description("Path to jacs.config.json")),
				mcp.WithString("hai_url", mcp.Description("HAI API URL override")),
			),
			Handler: handleAgentStatus,
		},
		{
			Tool: mcp.NewTool("hai_generate_verify_link",
				mcp.WithDescription("Generate a HAI verify link from a signed JACS document"),
				mcp.WithString("document", mcp.Required(), mcp.Description("Signed JACS document JSON string")),
				mcp.WithString("base_url", mcp.Description("Verifier base URL override")),
				mcp.WithBoolean("hosted", mcp.Description("Use hosted verify URL mode")),
			),
			Handler: handleGenerateVerifyLink,
		},
		{
			Tool: mcp.NewTool("hai_send_email",
				mcp.WithDescription("Send an email from the agent's @hai.ai address"),
				mcp.WithString("to", mcp.Required(), mcp.Description("Recipient email address")),
				mcp.WithString("subject", mcp.Required(), mcp.Description("Email subject line")),
				mcp.WithString("body", mcp.Required(), mcp.Description("Plain text email body")),
				mcp.WithString("in_reply_to", mcp.Description("Message-ID to reply to (for threading)")),
				mcp.WithString("config_path", mcp.Description("Path to jacs.config.json")),
				mcp.WithString("hai_url", mcp.Description("HAI API URL override")),
			),
			Handler: handleSendEmail,
		},
		{
			Tool: mcp.NewTool("hai_list_messages",
				mcp.WithDescription("List email messages in the agent's inbox/outbox"),
				mcp.WithNumber("limit", mcp.Description("Max messages to return (default 20)")),
				mcp.WithNumber("offset", mcp.Description("Pagination offset")),
				mcp.WithString("direction", mcp.Description("Filter: 'inbound' or 'outbound'")),
				mcp.WithString("config_path", mcp.Description("Path to jacs.config.json")),
				mcp.WithString("hai_url", mcp.Description("HAI API URL override")),
			),
			Handler: handleListMessages,
		},
		{
			Tool: mcp.NewTool("hai_get_message",
				mcp.WithDescription("Get a single email message by ID"),
				mcp.WithString("message_id", mcp.Required(), mcp.Description("Message UUID")),
				mcp.WithString("config_path", mcp.Description("Path to jacs.config.json")),
				mcp.WithString("hai_url", mcp.Description("HAI API URL override")),
			),
			Handler: handleGetMessage,
		},
		{
			Tool: mcp.NewTool("hai_delete_message",
				mcp.WithDescription("Delete an email message"),
				mcp.WithString("message_id", mcp.Required(), mcp.Description("Message UUID")),
				mcp.WithString("config_path", mcp.Description("Path to jacs.config.json")),
				mcp.WithString("hai_url", mcp.Description("HAI API URL override")),
			),
			Handler: handleDeleteMessage,
		},
		{
			Tool: mcp.NewTool("hai_mark_read",
				mcp.WithDescription("Mark an email message as read"),
				mcp.WithString("message_id", mcp.Required(), mcp.Description("Message UUID")),
				mcp.WithString("config_path", mcp.Description("Path to jacs.config.json")),
				mcp.WithString("hai_url", mcp.Description("HAI API URL override")),
			),
			Handler: handleMarkRead,
		},
		{
			Tool: mcp.NewTool("hai_mark_unread",
				mcp.WithDescription("Mark an email message as unread"),
				mcp.WithString("message_id", mcp.Required(), mcp.Description("Message UUID")),
				mcp.WithString("config_path", mcp.Description("Path to jacs.config.json")),
				mcp.WithString("hai_url", mcp.Description("HAI API URL override")),
			),
			Handler: handleMarkUnread,
		},
		{
			Tool: mcp.NewTool("hai_search_messages",
				mcp.WithDescription("Search email messages by query, sender, recipient, or date range"),
				mcp.WithString("q", mcp.Description("Search query text")),
				mcp.WithString("direction", mcp.Description("Filter: 'inbound' or 'outbound'")),
				mcp.WithString("from_address", mcp.Description("Filter by sender address")),
				mcp.WithString("to_address", mcp.Description("Filter by recipient address")),
				mcp.WithString("since", mcp.Description("Filter: messages after this ISO date")),
				mcp.WithString("until", mcp.Description("Filter: messages before this ISO date")),
				mcp.WithNumber("limit", mcp.Description("Max results (default 20)")),
				mcp.WithNumber("offset", mcp.Description("Pagination offset")),
				mcp.WithString("config_path", mcp.Description("Path to jacs.config.json")),
				mcp.WithString("hai_url", mcp.Description("HAI API URL override")),
			),
			Handler: handleSearchMessages,
		},
		{
			Tool: mcp.NewTool("hai_get_unread_count",
				mcp.WithDescription("Get the count of unread email messages"),
				mcp.WithString("config_path", mcp.Description("Path to jacs.config.json")),
				mcp.WithString("hai_url", mcp.Description("HAI API URL override")),
			),
			Handler: handleGetUnreadCount,
		},
		{
			Tool: mcp.NewTool("hai_get_email_status",
				mcp.WithDescription("Get email account status including usage limits and daily stats"),
				mcp.WithString("config_path", mcp.Description("Path to jacs.config.json")),
				mcp.WithString("hai_url", mcp.Description("HAI API URL override")),
			),
			Handler: handleGetEmailStatus,
		},
		{
			Tool: mcp.NewTool("hai_reply_email",
				mcp.WithDescription("Reply to an email message (fetches original, sends reply with threading)"),
				mcp.WithString("message_id", mcp.Required(), mcp.Description("ID of the message to reply to")),
				mcp.WithString("body", mcp.Required(), mcp.Description("Reply body text")),
				mcp.WithString("subject_override", mcp.Description("Override the Re: subject line")),
				mcp.WithString("config_path", mcp.Description("Path to jacs.config.json")),
				mcp.WithString("hai_url", mcp.Description("HAI API URL override")),
			),
			Handler: handleReplyEmail,
		},
	}
}

// ---------------------------------------------------------------------------
// Client helpers
// ---------------------------------------------------------------------------

func getClient(req mcp.CallToolRequest) (*haisdk.Client, error) {
	var opts []haisdk.Option

	if url := req.GetString("hai_url", ""); url != "" {
		opts = append(opts, haisdk.WithEndpoint(url))
	}

	// config_path is set via env var since Go SDK uses JACS_CONFIG_PATH
	if cp := req.GetString("config_path", ""); cp != "" {
		os.Setenv("JACS_CONFIG_PATH", cp)
		defer os.Unsetenv("JACS_CONFIG_PATH")
	}

	return haisdk.NewClient(opts...)
}

func toJSON(v interface{}) string {
	b, _ := json.Marshal(v)
	return string(b)
}

// ---------------------------------------------------------------------------
// Identity handlers
// ---------------------------------------------------------------------------

func handleHello(ctx context.Context, req mcp.CallToolRequest) (*mcp.CallToolResult, error) {
	client, err := getClient(req)
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	result, err := client.Hello(ctx)
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	return mcp.NewToolResultText(toJSON(result)), nil
}

func handleCheckUsername(ctx context.Context, req mcp.CallToolRequest) (*mcp.CallToolResult, error) {
	username, err := req.RequireString("username")
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	client, err := getClient(req)
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	result, err := client.CheckUsername(ctx, username)
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	return mcp.NewToolResultText(toJSON(result)), nil
}

func handleClaimUsername(ctx context.Context, req mcp.CallToolRequest) (*mcp.CallToolResult, error) {
	agentID, err := req.RequireString("agent_id")
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	username, err := req.RequireString("username")
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	client, err := getClient(req)
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	result, err := client.ClaimUsername(ctx, agentID, username)
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	return mcp.NewToolResultText(toJSON(result)), nil
}

func handleRegisterAgent(ctx context.Context, req mcp.CallToolRequest) (*mcp.CallToolResult, error) {
	client, err := getClient(req)
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	opts := haisdk.RegisterOptions{}
	if email := req.GetString("owner_email", ""); email != "" {
		opts.OwnerEmail = email
	}
	result, err := client.Register(ctx, opts)
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	return mcp.NewToolResultText(toJSON(result)), nil
}

func handleVerifyAgent(ctx context.Context, req mcp.CallToolRequest) (*mcp.CallToolResult, error) {
	agentID, err := req.RequireString("agent_id")
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	client, err := getClient(req)
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	result, err := client.VerifyAgent(ctx, agentID)
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	return mcp.NewToolResultText(toJSON(result)), nil
}

func handleAgentStatus(ctx context.Context, req mcp.CallToolRequest) (*mcp.CallToolResult, error) {
	client, err := getClient(req)
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	result, err := client.Status(ctx)
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	return mcp.NewToolResultText(toJSON(result)), nil
}

func handleGenerateVerifyLink(ctx context.Context, req mcp.CallToolRequest) (*mcp.CallToolResult, error) {
	document, err := req.RequireString("document")
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}

	baseURL := req.GetString("base_url", "")
	hosted := req.GetBool("hosted", false)

	var link string
	if hosted {
		link, err = haisdk.GenerateVerifyLinkHosted(document, baseURL)
	} else {
		link, err = haisdk.GenerateVerifyLink(document, baseURL)
	}
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}

	return mcp.NewToolResultText(toJSON(map[string]string{
		"verify_url": link,
	})), nil
}

// ---------------------------------------------------------------------------
// Email handlers
// ---------------------------------------------------------------------------

func handleSendEmail(ctx context.Context, req mcp.CallToolRequest) (*mcp.CallToolResult, error) {
	to, err := req.RequireString("to")
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	subject, err := req.RequireString("subject")
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	body, err := req.RequireString("body")
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	client, err := getClient(req)
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	opts := haisdk.SendEmailOptions{
		To:      to,
		Subject: subject,
		Body:    body,
	}
	if replyTo := req.GetString("in_reply_to", ""); replyTo != "" {
		opts.InReplyTo = replyTo
	}
	result, err := client.SendEmailWithOptions(ctx, opts)
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	return mcp.NewToolResultText(toJSON(result)), nil
}

func handleListMessages(ctx context.Context, req mcp.CallToolRequest) (*mcp.CallToolResult, error) {
	client, err := getClient(req)
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	opts := haisdk.ListMessagesOptions{
		Limit:     int(req.GetFloat("limit", 0)),
		Offset:    int(req.GetFloat("offset", 0)),
		Direction: req.GetString("direction", ""),
	}
	result, err := client.ListMessages(ctx, opts)
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	return mcp.NewToolResultText(toJSON(result)), nil
}

func handleGetMessage(ctx context.Context, req mcp.CallToolRequest) (*mcp.CallToolResult, error) {
	messageID, err := req.RequireString("message_id")
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	client, err := getClient(req)
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	result, err := client.GetMessage(ctx, messageID)
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	return mcp.NewToolResultText(toJSON(result)), nil
}

func handleDeleteMessage(ctx context.Context, req mcp.CallToolRequest) (*mcp.CallToolResult, error) {
	messageID, err := req.RequireString("message_id")
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	client, err := getClient(req)
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	if err := client.DeleteMessage(ctx, messageID); err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	return mcp.NewToolResultText(fmt.Sprintf(`{"deleted":true,"message_id":"%s"}`, messageID)), nil
}

func handleMarkRead(ctx context.Context, req mcp.CallToolRequest) (*mcp.CallToolResult, error) {
	messageID, err := req.RequireString("message_id")
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	client, err := getClient(req)
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	if err := client.MarkRead(ctx, messageID); err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	return mcp.NewToolResultText(fmt.Sprintf(`{"message_id":"%s","is_read":true}`, messageID)), nil
}

func handleMarkUnread(ctx context.Context, req mcp.CallToolRequest) (*mcp.CallToolResult, error) {
	messageID, err := req.RequireString("message_id")
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	client, err := getClient(req)
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	if err := client.MarkUnread(ctx, messageID); err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	return mcp.NewToolResultText(fmt.Sprintf(`{"message_id":"%s","is_read":false}`, messageID)), nil
}

func handleSearchMessages(ctx context.Context, req mcp.CallToolRequest) (*mcp.CallToolResult, error) {
	client, err := getClient(req)
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	opts := haisdk.SearchOptions{
		Q:           req.GetString("q", ""),
		Direction:   req.GetString("direction", ""),
		FromAddress: req.GetString("from_address", ""),
		ToAddress:   req.GetString("to_address", ""),
		Limit:       int(req.GetFloat("limit", 0)),
		Offset:      int(req.GetFloat("offset", 0)),
	}
	result, err := client.SearchMessages(ctx, opts)
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	return mcp.NewToolResultText(toJSON(result)), nil
}

func handleGetUnreadCount(ctx context.Context, req mcp.CallToolRequest) (*mcp.CallToolResult, error) {
	client, err := getClient(req)
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	count, err := client.GetUnreadCount(ctx)
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	return mcp.NewToolResultText(fmt.Sprintf(`{"count":%d}`, count)), nil
}

func handleGetEmailStatus(ctx context.Context, req mcp.CallToolRequest) (*mcp.CallToolResult, error) {
	client, err := getClient(req)
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	result, err := client.GetEmailStatus(ctx)
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	return mcp.NewToolResultText(toJSON(result)), nil
}

func handleReplyEmail(ctx context.Context, req mcp.CallToolRequest) (*mcp.CallToolResult, error) {
	messageID, err := req.RequireString("message_id")
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	body, err := req.RequireString("body")
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	subjectOverride := req.GetString("subject_override", "")
	client, err := getClient(req)
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	result, err := client.Reply(ctx, messageID, body, subjectOverride)
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	return mcp.NewToolResultText(toJSON(result)), nil
}
