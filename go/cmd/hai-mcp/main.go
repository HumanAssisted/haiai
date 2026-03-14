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

	haiai "github.com/HumanAssisted/haiai-go"
	"github.com/mark3labs/mcp-go/mcp"
	"github.com/mark3labs/mcp-go/server"
)

func main() {
	s := server.NewMCPServer(
		"haiai",
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
				mcp.WithArray("cc", mcp.Description("CC recipient addresses")),
				mcp.WithArray("bcc", mcp.Description("BCC recipient addresses")),
				mcp.WithArray("labels", mcp.Description("Labels/tags for the message")),
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
				mcp.WithBoolean("is_read", mcp.Description("Filter by read status")),
				mcp.WithString("folder", mcp.Description("Filter by folder: 'inbox' or 'archive'")),
				mcp.WithString("label", mcp.Description("Filter by label")),
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
				mcp.WithBoolean("is_read", mcp.Description("Filter by read status")),
				mcp.WithBoolean("jacs_verified", mcp.Description("Filter by JACS verification status")),
				mcp.WithString("folder", mcp.Description("Filter by folder: 'inbox' or 'archive'")),
				mcp.WithString("label", mcp.Description("Filter by label")),
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
		{
			Tool: mcp.NewTool("hai_forward_email",
				mcp.WithDescription("Forward an email message to another recipient"),
				mcp.WithString("message_id", mcp.Required(), mcp.Description("ID of the message to forward")),
				mcp.WithString("to", mcp.Required(), mcp.Description("Recipient email address to forward to")),
				mcp.WithString("comment", mcp.Description("Optional comment to prepend")),
				mcp.WithString("config_path", mcp.Description("Path to jacs.config.json")),
				mcp.WithString("hai_url", mcp.Description("HAI API URL override")),
			),
			Handler: handleForwardEmail,
		},
		{
			Tool: mcp.NewTool("hai_archive_message",
				mcp.WithDescription("Archive an email message"),
				mcp.WithString("message_id", mcp.Required(), mcp.Description("Message UUID")),
				mcp.WithString("config_path", mcp.Description("Path to jacs.config.json")),
				mcp.WithString("hai_url", mcp.Description("HAI API URL override")),
			),
			Handler: handleArchiveMessage,
		},
		{
			Tool: mcp.NewTool("hai_unarchive_message",
				mcp.WithDescription("Unarchive (restore) an email message to the inbox"),
				mcp.WithString("message_id", mcp.Required(), mcp.Description("Message UUID")),
				mcp.WithString("config_path", mcp.Description("Path to jacs.config.json")),
				mcp.WithString("hai_url", mcp.Description("HAI API URL override")),
			),
			Handler: handleUnarchiveMessage,
		},
		{
			Tool: mcp.NewTool("hai_list_contacts",
				mcp.WithDescription("List contacts derived from email history"),
				mcp.WithString("config_path", mcp.Description("Path to jacs.config.json")),
				mcp.WithString("hai_url", mcp.Description("HAI API URL override")),
			),
			Handler: handleListContacts,
		},
	}
}

// ---------------------------------------------------------------------------
// Client helpers
// ---------------------------------------------------------------------------

func getClient(req mcp.CallToolRequest) (*haiai.Client, error) {
	var opts []haiai.Option

	if url := req.GetString("hai_url", ""); url != "" {
		opts = append(opts, haiai.WithEndpoint(url))
	}

	// config_path is set via env var since Go SDK uses JACS_CONFIG_PATH
	if cp := req.GetString("config_path", ""); cp != "" {
		os.Setenv("JACS_CONFIG_PATH", cp)
		defer os.Unsetenv("JACS_CONFIG_PATH")
	}

	return haiai.NewClient(opts...)
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
	opts := haiai.RegisterOptions{}
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
		link, err = haiai.GenerateVerifyLinkHosted(document, baseURL)
	} else {
		link, err = haiai.GenerateVerifyLink(document, baseURL)
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
	opts := haiai.SendEmailOptions{
		To:      to,
		Subject: subject,
		Body:    body,
	}
	if replyTo := req.GetString("in_reply_to", ""); replyTo != "" {
		opts.InReplyTo = replyTo
	}
	if cc := req.GetStringSlice("cc", nil); len(cc) > 0 {
		opts.CC = cc
	}
	if bcc := req.GetStringSlice("bcc", nil); len(bcc) > 0 {
		opts.BCC = bcc
	}
	if labels := req.GetStringSlice("labels", nil); len(labels) > 0 {
		opts.Labels = labels
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
	opts := haiai.ListMessagesOptions{
		Limit:     int(req.GetFloat("limit", 0)),
		Offset:    int(req.GetFloat("offset", 0)),
		Direction: req.GetString("direction", ""),
		Folder:    req.GetString("folder", ""),
		Label:     req.GetString("label", ""),
	}
	if hasBoolArg(req, "is_read") {
		v := req.GetBool("is_read", false)
		opts.IsRead = &v
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
	opts := haiai.SearchOptions{
		Q:           req.GetString("q", ""),
		Direction:   req.GetString("direction", ""),
		FromAddress: req.GetString("from_address", ""),
		ToAddress:   req.GetString("to_address", ""),
		Limit:       int(req.GetFloat("limit", 0)),
		Offset:      int(req.GetFloat("offset", 0)),
		Folder:      req.GetString("folder", ""),
		Label:       req.GetString("label", ""),
	}
	if hasBoolArg(req, "is_read") {
		v := req.GetBool("is_read", false)
		opts.IsRead = &v
	}
	if hasBoolArg(req, "jacs_verified") {
		v := req.GetBool("jacs_verified", false)
		opts.JacsVerified = &v
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

func handleForwardEmail(ctx context.Context, req mcp.CallToolRequest) (*mcp.CallToolResult, error) {
	messageID, err := req.RequireString("message_id")
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	to, err := req.RequireString("to")
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	client, err := getClient(req)
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	opts := haiai.ForwardOptions{
		MessageID: messageID,
		To:        to,
		Comment:   req.GetString("comment", ""),
	}
	result, err := client.Forward(ctx, opts)
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	return mcp.NewToolResultText(toJSON(result)), nil
}

func handleArchiveMessage(ctx context.Context, req mcp.CallToolRequest) (*mcp.CallToolResult, error) {
	messageID, err := req.RequireString("message_id")
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	client, err := getClient(req)
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	if err := client.Archive(ctx, messageID); err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	return mcp.NewToolResultText(fmt.Sprintf(`{"archived":true,"message_id":"%s"}`, messageID)), nil
}

func handleUnarchiveMessage(ctx context.Context, req mcp.CallToolRequest) (*mcp.CallToolResult, error) {
	messageID, err := req.RequireString("message_id")
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	client, err := getClient(req)
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	if err := client.Unarchive(ctx, messageID); err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	return mcp.NewToolResultText(fmt.Sprintf(`{"unarchived":true,"message_id":"%s"}`, messageID)), nil
}

func handleListContacts(ctx context.Context, req mcp.CallToolRequest) (*mcp.CallToolResult, error) {
	client, err := getClient(req)
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	result, err := client.GetContacts(ctx)
	if err != nil {
		return mcp.NewToolResultError(err.Error()), nil
	}
	return mcp.NewToolResultText(toJSON(result)), nil
}

// hasBoolArg checks if a boolean argument was explicitly provided in the request.
// GetBool always returns a default, so we need this to distinguish "not set" from "set to false".
func hasBoolArg(req mcp.CallToolRequest, key string) bool {
	args := req.GetArguments()
	if args == nil {
		return false
	}
	_, exists := args[key]
	return exists
}
