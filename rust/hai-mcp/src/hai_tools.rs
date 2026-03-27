use haiai::{
    generate_verify_link, generate_verify_link_hosted, CreateEmailTemplateOptions, HaiClient,
    JacsProvider, ListEmailTemplatesOptions, ListMessagesOptions, RegisterAgentOptions,
    SearchOptions, SendEmailOptions, UpdateEmailTemplateOptions,
};
use rmcp::model::{CallToolResult, Content, JsonObject, Tool};
use rmcp::ErrorData as McpError;
use serde_json::{json, Value};

use crate::context::HaiServerContext;

#[derive(Debug)]
enum ToolError {
    InvalidParams(String),
    Message(String),
}

type ToolResult = Result<CallToolResult, ToolError>;

fn tool_message<E: std::fmt::Display>(error: E) -> ToolError {
    ToolError::Message(error.to_string())
}

pub fn has_tool(name: &str) -> bool {
    matches!(
        name,
        "hai_check_username"
            | "hai_hello"
            | "hai_agent_status"
            | "hai_verify_status"
            | "hai_claim_username"
            | "hai_register_agent"
            | "hai_generate_verify_link"
            | "hai_send_email"
            | "hai_list_messages"
            | "hai_get_message"
            | "hai_delete_message"
            | "hai_mark_read"
            | "hai_mark_unread"
            | "hai_search_messages"
            | "hai_get_unread_count"
            | "hai_get_email_status"
            | "hai_reply_email"
            | "hai_forward_email"
            | "hai_archive_message"
            | "hai_unarchive_message"
            | "hai_list_contacts"
            | "hai_self_knowledge"
            | "hai_create_email_template"
            | "hai_list_email_templates"
            | "hai_search_email_templates"
            | "hai_get_email_template"
            | "hai_update_email_template"
            | "hai_delete_email_template"
    )
}

pub fn definitions() -> Vec<Tool> {
    definition_values()
        .into_iter()
        .map(|value| serde_json::from_value(value).expect("valid HAI tool definition"))
        .collect()
}

pub async fn dispatch(
    context: &HaiServerContext,
    name: &str,
    arguments: Option<JsonObject>,
) -> Result<CallToolResult, McpError> {
    let args = Value::Object(arguments.unwrap_or_default());

    let result = match name {
        "hai_check_username" => call_check_username(context, &args).await,
        "hai_hello" => call_hello(context, &args).await,
        "hai_agent_status" => call_verify_status(context, &args).await,
        "hai_verify_status" => call_verify_status(context, &args).await,
        "hai_claim_username" => call_claim_username(context, &args).await,
        "hai_register_agent" => call_register_agent(context, &args).await,
        "hai_generate_verify_link" => call_generate_verify_link(&args).await,
        "hai_send_email" => call_send_email(context, &args).await,
        "hai_list_messages" => call_list_messages(context, &args).await,
        "hai_get_message" => call_get_message(context, &args).await,
        "hai_delete_message" => call_delete_message(context, &args).await,
        "hai_mark_read" => call_mark_read(context, &args).await,
        "hai_mark_unread" => call_mark_unread(context, &args).await,
        "hai_search_messages" => call_search_messages(context, &args).await,
        "hai_get_unread_count" => call_get_unread_count(context, &args).await,
        "hai_get_email_status" => call_get_email_status(context, &args).await,
        "hai_reply_email" => call_reply_email(context, &args).await,
        "hai_forward_email" => call_forward_email(context, &args).await,
        "hai_archive_message" => call_archive_message(context, &args).await,
        "hai_unarchive_message" => call_unarchive_message(context, &args).await,
        "hai_list_contacts" => call_list_contacts(context, &args).await,
        "hai_self_knowledge" => call_self_knowledge(&args).await,
        "hai_create_email_template" => call_create_email_template(context, &args).await,
        "hai_list_email_templates" => call_list_email_templates(context, &args).await,
        "hai_search_email_templates" => call_list_email_templates(context, &args).await,
        "hai_get_email_template" => call_get_email_template(context, &args).await,
        "hai_update_email_template" => call_update_email_template(context, &args).await,
        "hai_delete_email_template" => call_delete_email_template(context, &args).await,
        _ => Err(ToolError::InvalidParams(format!(
            "unknown HAI tool: {name}"
        ))),
    };

    match result {
        Ok(result) => Ok(result),
        Err(ToolError::InvalidParams(message)) => Err(McpError::invalid_params(message, None)),
        Err(ToolError::Message(message)) => Ok(error_tool_result(message)),
    }
}

fn definition_values() -> Vec<Value> {
    vec![
        json!({
            "name": "hai_check_username",
            "description": "Check if a hai.ai username is available",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "username": { "type": "string" }
                },
                "required": ["username"]
            }
        }),
        json!({
            "name": "hai_hello",
            "description": "Run authenticated hello handshake with HAI using local JACS config",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "include_test": { "type": "boolean" },
                    "config_path": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "hai_agent_status",
            "description": "Get the current agent's verification status",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "config_path": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "hai_verify_status",
            "description": "Get verification status for the current or provided agent",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string" },
                    "config_path": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "hai_claim_username",
            "description": "Claim a username for an agent ID",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string" },
                    "username": { "type": "string" },
                    "config_path": { "type": "string" }
                },
                "required": ["agent_id", "username"]
            }
        }),
        json!({
            "name": "hai_register_agent",
            "description": "Register an existing local JACS agent with HAI",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "config_path": { "type": "string" },
                    "owner_email": { "type": "string" },
                    "domain": { "type": "string" },
                    "description": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "hai_generate_verify_link",
            "description": "Generate a HAI verify link from a signed JACS document",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "document": { "type": "string" },
                    "base_url": { "type": "string" },
                    "hosted": { "type": "boolean" }
                },
                "required": ["document"]
            }
        }),
        json!({
            "name": "hai_send_email",
            "description": "Send an email from the agent's @hai.ai address. Tip: use hai_search_email_templates first to find a template with instructions and rules for this type of email.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "to": { "type": "string", "description": "Recipient email address" },
                    "subject": { "type": "string", "description": "Email subject line" },
                    "body": { "type": "string", "description": "Plain text email body" },
                    "cc": { "type": "array", "items": { "type": "string" }, "description": "CC recipients" },
                    "bcc": { "type": "array", "items": { "type": "string" }, "description": "BCC recipients" },
                    "labels": { "type": "array", "items": { "type": "string" }, "description": "Labels/tags for the message" },
                    "in_reply_to": { "type": "string", "description": "Message-ID to reply to (for threading)" },
                    "agent_id": { "type": "string", "description": "Optional HAI agent UUID for stateless MCP sessions" },
                    "config_path": { "type": "string" }
                },
                "required": ["to", "subject", "body"]
            }
        }),
        json!({
            "name": "hai_list_messages",
            "description": "List email messages in the agent's inbox/outbox",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "description": "Max messages to return (default 20)" },
                    "offset": { "type": "integer", "description": "Pagination offset" },
                    "direction": { "type": "string", "description": "Filter: 'inbound' or 'outbound'" },
                    "is_read": { "type": "boolean", "description": "Filter by read status" },
                    "folder": { "type": "string", "description": "Filter by folder (e.g. 'inbox', 'archive')" },
                    "label": { "type": "string", "description": "Filter by label/tag" },
                    "has_attachments": { "type": "boolean", "description": "Filter by attachment presence" },
                    "agent_id": { "type": "string", "description": "Optional HAI agent UUID for stateless MCP sessions" },
                    "config_path": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "hai_get_message",
            "description": "Get a single email message by ID",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "message_id": { "type": "string", "description": "Message UUID" },
                    "agent_id": { "type": "string", "description": "Optional HAI agent UUID for stateless MCP sessions" },
                    "config_path": { "type": "string" }
                },
                "required": ["message_id"]
            }
        }),
        json!({
            "name": "hai_delete_message",
            "description": "Delete an email message",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "message_id": { "type": "string", "description": "Message UUID" },
                    "agent_id": { "type": "string", "description": "Optional HAI agent UUID for stateless MCP sessions" },
                    "config_path": { "type": "string" }
                },
                "required": ["message_id"]
            }
        }),
        json!({
            "name": "hai_mark_read",
            "description": "Mark an email message as read",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "message_id": { "type": "string", "description": "Message UUID" },
                    "agent_id": { "type": "string", "description": "Optional HAI agent UUID for stateless MCP sessions" },
                    "config_path": { "type": "string" }
                },
                "required": ["message_id"]
            }
        }),
        json!({
            "name": "hai_mark_unread",
            "description": "Mark an email message as unread",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "message_id": { "type": "string", "description": "Message UUID" },
                    "agent_id": { "type": "string", "description": "Optional HAI agent UUID for stateless MCP sessions" },
                    "config_path": { "type": "string" }
                },
                "required": ["message_id"]
            }
        }),
        json!({
            "name": "hai_search_messages",
            "description": "Search email messages by query, sender, recipient, or date range",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "q": { "type": "string", "description": "Search query text" },
                    "direction": { "type": "string", "description": "Filter: 'inbound' or 'outbound'" },
                    "from_address": { "type": "string", "description": "Filter by sender address" },
                    "to_address": { "type": "string", "description": "Filter by recipient address" },
                    "since": { "type": "string", "description": "Filter: messages after this ISO date" },
                    "until": { "type": "string", "description": "Filter: messages before this ISO date" },
                    "is_read": { "type": "boolean", "description": "Filter by read status" },
                    "jacs_verified": { "type": "boolean", "description": "Filter by JACS verification status" },
                    "folder": { "type": "string", "description": "Filter by folder (e.g. 'inbox', 'archive')" },
                    "label": { "type": "string", "description": "Filter by label/tag" },
                    "has_attachments": { "type": "boolean", "description": "Filter by attachment presence" },
                    "limit": { "type": "integer", "description": "Max results (default 20)" },
                    "offset": { "type": "integer", "description": "Pagination offset" },
                    "agent_id": { "type": "string", "description": "Optional HAI agent UUID for stateless MCP sessions" },
                    "config_path": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "hai_get_unread_count",
            "description": "Get the count of unread email messages",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string", "description": "Optional HAI agent UUID for stateless MCP sessions" },
                    "config_path": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "hai_get_email_status",
            "description": "Get email account status including usage limits and daily stats",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string", "description": "Optional HAI agent UUID for stateless MCP sessions" },
                    "config_path": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "hai_reply_email",
            "description": "Reply to an email message (fetches original, sends reply with threading). Tip: use hai_search_email_templates first to find a template with how_to_respond instructions for this type of email.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "message_id": { "type": "string", "description": "ID of the message to reply to" },
                    "body": { "type": "string", "description": "Reply body text" },
                    "subject_override": { "type": "string", "description": "Override the Re: subject line" },
                    "agent_id": { "type": "string", "description": "Optional HAI agent UUID for stateless MCP sessions" },
                    "config_path": { "type": "string" }
                },
                "required": ["message_id", "body"]
            }
        }),
        json!({
            "name": "hai_forward_email",
            "description": "Forward an email message to another recipient",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "message_id": { "type": "string", "description": "ID of the message to forward" },
                    "to": { "type": "string", "description": "Recipient email address" },
                    "comment": { "type": "string", "description": "Optional comment to include above the forwarded message" },
                    "agent_id": { "type": "string", "description": "Optional HAI agent UUID for stateless MCP sessions" },
                    "config_path": { "type": "string" }
                },
                "required": ["message_id", "to"]
            }
        }),
        json!({
            "name": "hai_archive_message",
            "description": "Archive an email message (move to archive folder)",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "message_id": { "type": "string", "description": "Message UUID to archive" },
                    "agent_id": { "type": "string", "description": "Optional HAI agent UUID for stateless MCP sessions" },
                    "config_path": { "type": "string" }
                },
                "required": ["message_id"]
            }
        }),
        json!({
            "name": "hai_unarchive_message",
            "description": "Unarchive an email message (move back to inbox)",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "message_id": { "type": "string", "description": "Message UUID to unarchive" },
                    "agent_id": { "type": "string", "description": "Optional HAI agent UUID for stateless MCP sessions" },
                    "config_path": { "type": "string" }
                },
                "required": ["message_id"]
            }
        }),
        json!({
            "name": "hai_list_contacts",
            "description": "List email contacts derived from message history, enriched with HAI metadata",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string", "description": "Optional HAI agent UUID for stateless MCP sessions" },
                    "config_path": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "hai_self_knowledge",
            "description": "Search embedded JACS and HAI documentation. Use this to look up how signing, verification, email, key rotation, A2A, schemas, storage, and other concepts work. Returns ranked chapters with full text.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Natural language or keyword search query"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max results (default 5)"
                    }
                },
                "required": ["query"]
            }
        }),
        // =====================================================================
        // Email Template Tools
        // =====================================================================
        json!({
            "name": "hai_create_email_template",
            "description": "Create a reusable email template with instructions for sending and responding. Use templates to ensure consistency for repeated email types. Fields: name (unique label), how_to_send (composition instructions), how_to_respond (reply handling), goal (what the email should achieve), rules (guardrails like 'no PII').",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Unique template name (e.g. 'Cold Outreach', 'Support Reply')" },
                    "how_to_send": { "type": "string", "description": "Instructions for composing this type of email (tone, structure, personalization)" },
                    "how_to_respond": { "type": "string", "description": "Instructions for replying to this type of email (handling positive/negative/questions)" },
                    "goal": { "type": "string", "description": "The objective this template serves" },
                    "rules": { "type": "string", "description": "Constraints: e.g. 'no PII', 'don't send to @competitor.com', 'keep under 200 words'" },
                    "agent_id": { "type": "string", "description": "Optional HAI agent UUID for stateless MCP sessions" },
                    "config_path": { "type": "string" }
                },
                "required": ["name"]
            }
        }),
        json!({
            "name": "hai_list_email_templates",
            "description": "List your email templates. Use this to review available templates before composing or replying to email. Supports search via the 'q' parameter to find the most relevant template for a given situation.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "q": { "type": "string", "description": "Search query to find relevant templates" },
                    "limit": { "type": "integer", "description": "Max results (default 50)" },
                    "offset": { "type": "integer", "description": "Pagination offset" },
                    "agent_id": { "type": "string", "description": "Optional HAI agent UUID for stateless MCP sessions" },
                    "config_path": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "hai_search_email_templates",
            "description": "Search email templates by keyword. **Before sending or replying to an email, search your templates to find relevant instructions.** This ensures you follow established patterns and rules for this type of communication.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "q": { "type": "string", "description": "Search query (e.g. 'outreach', 'support', 'follow-up')" },
                    "limit": { "type": "integer", "description": "Max results (default 50)" },
                    "offset": { "type": "integer", "description": "Pagination offset" },
                    "agent_id": { "type": "string", "description": "Optional HAI agent UUID for stateless MCP sessions" },
                    "config_path": { "type": "string" }
                },
                "required": ["q"]
            }
        }),
        json!({
            "name": "hai_get_email_template",
            "description": "Get a specific email template by ID. Read the full template before composing an email to follow its how_to_send, how_to_respond, goal, and rules.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "template_id": { "type": "string", "description": "Template UUID" },
                    "agent_id": { "type": "string", "description": "Optional HAI agent UUID for stateless MCP sessions" },
                    "config_path": { "type": "string" }
                },
                "required": ["template_id"]
            }
        }),
        json!({
            "name": "hai_update_email_template",
            "description": "Update an existing email template. Use this to refine instructions as email patterns evolve.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "template_id": { "type": "string", "description": "Template UUID to update" },
                    "name": { "type": "string", "description": "New template name" },
                    "how_to_send": { "type": "string", "description": "Updated composition instructions" },
                    "how_to_respond": { "type": "string", "description": "Updated reply instructions" },
                    "goal": { "type": "string", "description": "Updated goal" },
                    "rules": { "type": "string", "description": "Updated rules/constraints" },
                    "agent_id": { "type": "string", "description": "Optional HAI agent UUID for stateless MCP sessions" },
                    "config_path": { "type": "string" }
                },
                "required": ["template_id"]
            }
        }),
        json!({
            "name": "hai_delete_email_template",
            "description": "Delete an email template (soft delete). Use when a template is no longer needed.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "template_id": { "type": "string", "description": "Template UUID to delete" },
                    "agent_id": { "type": "string", "description": "Optional HAI agent UUID for stateless MCP sessions" },
                    "config_path": { "type": "string" }
                },
                "required": ["template_id"]
            }
        }),
    ]
}

async fn call_check_username(context: &HaiServerContext, args: &Value) -> ToolResult {
    let username = required_string(args, "username")?;
    let hai_url = optional_string(args, "hai_url");

    let client = context
        .noop_client_with_url(hai_url)
        .map_err(tool_message)?;
    let result = client
        .check_username(username)
        .await
        .map_err(tool_message)?;

    Ok(success_tool_result(
        format!(
            "username={} available={} reason={}",
            result.username,
            result.available,
            result.reason.clone().unwrap_or_default()
        ),
        json!({ "check_username": result }),
    ))
}

async fn call_hello(context: &HaiServerContext, args: &Value) -> ToolResult {
    let include_test = args
        .get("include_test")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let config_path = optional_string(args, "config_path");
    let hai_url = optional_string(args, "hai_url");

    let client = context
        .embedded_client_with_url(config_path, hai_url)
        .map_err(tool_message)?;
    let result = client.hello(include_test).await.map_err(tool_message)?;

    Ok(success_tool_result(
        format!("hello_id={} message={}", result.hello_id, result.message),
        json!({ "hello": result }),
    ))
}

async fn call_verify_status(context: &HaiServerContext, args: &Value) -> ToolResult {
    let agent_id = optional_string(args, "agent_id");
    let config_path = optional_string(args, "config_path");
    let hai_url = optional_string(args, "hai_url");

    let client = context
        .embedded_client_with_url(config_path, hai_url)
        .map_err(tool_message)?;
    let result = client.verify_status(agent_id).await.map_err(tool_message)?;

    Ok(success_tool_result(
        format!(
            "jacs_id={} registered={} dns_verified={}",
            result.jacs_id, result.registered, result.dns_verified
        ),
        json!({ "verify_status": result }),
    ))
}

async fn call_claim_username(context: &HaiServerContext, args: &Value) -> ToolResult {
    let agent_id = required_string(args, "agent_id")?;
    let username = required_string(args, "username")?;
    let config_path = optional_string(args, "config_path");
    let hai_url = optional_string(args, "hai_url");

    let mut client = context
        .embedded_client_with_url(config_path, hai_url)
        .map_err(tool_message)?;
    let result = client
        .claim_username(agent_id, username)
        .await
        .map_err(tool_message)?;
    let jacs_id = client.jacs_id().to_string();
    context.remember_hai_agent_id(&jacs_id, &result.agent_id);
    context.remember_agent_email(&jacs_id, &result.email);

    Ok(success_tool_result(
        format!(
            "claimed username={} for agent_id={}",
            result.username, result.agent_id
        ),
        json!({ "claim_username": result }),
    ))
}

async fn call_register_agent(context: &HaiServerContext, args: &Value) -> ToolResult {
    let config_path = optional_string(args, "config_path");
    let provider = context
        .embedded_provider(config_path)
        .map_err(tool_message)?;
    let jacs_id = provider.jacs_id().to_string();

    let agent_json = provider.export_agent_json().map_err(tool_message)?;
    let public_key_pem = provider.public_key_pem().map_err(tool_message)?;

    let client = context
        .client_with_provider(provider, optional_string(args, "hai_url"))
        .map_err(tool_message)?;
    let result = client
        .register(&RegisterAgentOptions {
            agent_json,
            public_key_pem: Some(public_key_pem),
            owner_email: optional_string(args, "owner_email").map(ToString::to_string),
            domain: optional_string(args, "domain").map(ToString::to_string),
            description: optional_string(args, "description").map(ToString::to_string),
        })
        .await
        .map_err(tool_message)?;
    context.remember_hai_agent_id(&jacs_id, &result.agent_id);

    Ok(success_tool_result(
        format!(
            "registered jacs_id={} agent_id={}",
            result.jacs_id, result.agent_id
        ),
        json!({ "registration": result }),
    ))
}

async fn call_generate_verify_link(args: &Value) -> ToolResult {
    let document = required_string(args, "document")?;
    let base_url = optional_string(args, "base_url");
    let hosted = args.get("hosted").and_then(Value::as_bool).unwrap_or(false);

    let url = if hosted {
        generate_verify_link_hosted(document, base_url)
    } else {
        generate_verify_link(document, base_url)
    }
    .map_err(tool_message)?;

    Ok(success_tool_result(
        format!("verify_url={url}"),
        json!({ "verify_url": url }),
    ))
}

fn apply_email_identity_overrides(
    context: &HaiServerContext,
    client: &mut HaiClient<impl JacsProvider>,
    args: &Value,
) -> Result<(), ToolError> {
    let jacs_id = client.jacs_id().to_string();

    if let Some(agent_id) = optional_string(args, "agent_id").filter(|value| !value.is_empty()) {
        if let Some(cached_agent_id) = context.cached_hai_agent_id(&jacs_id) {
            if cached_agent_id != agent_id {
                return Err(ToolError::InvalidParams(
                    "agent_id does not match the cached HAI identity for this JACS agent"
                        .to_string(),
                ));
            }
        } else {
            client.set_hai_agent_id(agent_id.to_string());
        }
    }

    if let Some(email) = optional_string(args, "email").filter(|value| !value.is_empty()) {
        if context.cached_agent_email(&jacs_id).as_deref() != Some(email) {
            return Err(ToolError::InvalidParams(
                "email overrides are not supported; derive the address from HAI status or username claim"
                    .to_string(),
            ));
        }
    }

    Ok(())
}

async fn prepare_email_client(
    context: &HaiServerContext,
    args: &Value,
) -> Result<HaiClient<impl JacsProvider>, ToolError> {
    let mut client = context
        .embedded_client_with_url(
            optional_string(args, "config_path"),
            optional_string(args, "hai_url"),
        )
        .map_err(tool_message)?;
    apply_email_identity_overrides(context, &mut client, args)?;

    if client.agent_email().is_none() {
        if let Ok(status) = client.get_email_status().await {
            if !status.email.is_empty() {
                context.remember_agent_email(client.jacs_id(), &status.email);
                client.set_agent_email(status.email);
            }
        }
    }

    Ok(client)
}

async fn call_send_email(context: &HaiServerContext, args: &Value) -> ToolResult {
    let client = prepare_email_client(context, args).await?;
    let result = client
        .send_signed_email(&SendEmailOptions {
            to: required_string(args, "to")?.to_string(),
            subject: required_string(args, "subject")?.to_string(),
            body: required_string(args, "body")?.to_string(),
            cc: optional_string_array(args, "cc"),
            bcc: optional_string_array(args, "bcc"),
            in_reply_to: optional_string(args, "in_reply_to").map(ToString::to_string),
            attachments: vec![],
            labels: optional_string_array(args, "labels"),
            append_footer: None,
        })
        .await
        .map_err(tool_message)?;

    Ok(success_tool_result(
        format!(
            "sent message_id={} status={}",
            result.message_id, result.status
        ),
        json!({ "send_email": result }),
    ))
}

async fn call_list_messages(context: &HaiServerContext, args: &Value) -> ToolResult {
    let client = prepare_email_client(context, args).await?;
    let result = client
        .list_messages(&ListMessagesOptions {
            limit: optional_u32(args, "limit"),
            offset: optional_u32(args, "offset"),
            direction: optional_string(args, "direction").map(ToString::to_string),
            is_read: optional_bool(args, "is_read"),
            folder: optional_string(args, "folder").map(ToString::to_string),
            label: optional_string(args, "label").map(ToString::to_string),
            has_attachments: optional_bool(args, "has_attachments"),
        })
        .await
        .map_err(tool_message)?;

    let count = result.len();
    Ok(success_tool_result(
        format!("found {count} messages"),
        json!({ "messages": result }),
    ))
}

async fn call_get_message(context: &HaiServerContext, args: &Value) -> ToolResult {
    let message_id = required_string(args, "message_id")?;
    let client = prepare_email_client(context, args).await?;
    let result = client.get_message(message_id).await.map_err(tool_message)?;

    Ok(success_tool_result(
        format!(
            "message from={} to={} subject={}",
            result.from_address, result.to_address, result.subject
        ),
        json!({ "message": result }),
    ))
}

async fn call_delete_message(context: &HaiServerContext, args: &Value) -> ToolResult {
    let message_id = required_string(args, "message_id")?;
    let client = prepare_email_client(context, args).await?;
    client
        .delete_message(message_id)
        .await
        .map_err(tool_message)?;

    Ok(success_tool_result(
        format!("deleted message_id={message_id}"),
        json!({ "deleted": true, "message_id": message_id }),
    ))
}

async fn call_mark_read(context: &HaiServerContext, args: &Value) -> ToolResult {
    let message_id = required_string(args, "message_id")?;
    let client = prepare_email_client(context, args).await?;
    client.mark_read(message_id).await.map_err(tool_message)?;

    Ok(success_tool_result(
        format!("marked read message_id={message_id}"),
        json!({ "message_id": message_id, "is_read": true }),
    ))
}

async fn call_mark_unread(context: &HaiServerContext, args: &Value) -> ToolResult {
    let message_id = required_string(args, "message_id")?;
    let client = prepare_email_client(context, args).await?;
    client.mark_unread(message_id).await.map_err(tool_message)?;

    Ok(success_tool_result(
        format!("marked unread message_id={message_id}"),
        json!({ "message_id": message_id, "is_read": false }),
    ))
}

async fn call_search_messages(context: &HaiServerContext, args: &Value) -> ToolResult {
    let client = prepare_email_client(context, args).await?;
    let result = client
        .search_messages(&SearchOptions {
            q: optional_string(args, "q").map(ToString::to_string),
            direction: optional_string(args, "direction").map(ToString::to_string),
            from_address: optional_string(args, "from_address").map(ToString::to_string),
            to_address: optional_string(args, "to_address").map(ToString::to_string),
            since: optional_string(args, "since").map(ToString::to_string),
            until: optional_string(args, "until").map(ToString::to_string),
            is_read: optional_bool(args, "is_read"),
            jacs_verified: optional_bool(args, "jacs_verified"),
            folder: optional_string(args, "folder").map(ToString::to_string),
            label: optional_string(args, "label").map(ToString::to_string),
            has_attachments: optional_bool(args, "has_attachments"),
            limit: optional_u32(args, "limit"),
            offset: optional_u32(args, "offset"),
        })
        .await
        .map_err(tool_message)?;

    let count = result.len();
    Ok(success_tool_result(
        format!("found {count} messages"),
        json!({ "messages": result }),
    ))
}

async fn call_get_unread_count(context: &HaiServerContext, args: &Value) -> ToolResult {
    let client = prepare_email_client(context, args).await?;
    let count = client.get_unread_count().await.map_err(tool_message)?;

    Ok(success_tool_result(
        format!("unread_count={count}"),
        json!({ "count": count }),
    ))
}

async fn call_get_email_status(context: &HaiServerContext, args: &Value) -> ToolResult {
    let client = prepare_email_client(context, args).await?;
    let result = client.get_email_status().await.map_err(tool_message)?;
    context.remember_agent_email(client.jacs_id(), &result.email);

    Ok(success_tool_result(
        format!(
            "email={} status={} tier={} daily_used={}/{}",
            result.email, result.status, result.tier, result.daily_used, result.daily_limit
        ),
        json!({ "email_status": result }),
    ))
}

async fn call_reply_email(context: &HaiServerContext, args: &Value) -> ToolResult {
    let message_id = required_string(args, "message_id")?;
    let body = required_string(args, "body")?;
    let subject_override = optional_string(args, "subject_override");
    let client = prepare_email_client(context, args).await?;
    let result = client
        .reply(message_id, body, subject_override)
        .await
        .map_err(tool_message)?;

    Ok(success_tool_result(
        format!(
            "replied message_id={} status={}",
            result.message_id, result.status
        ),
        json!({ "reply": result }),
    ))
}

async fn call_forward_email(context: &HaiServerContext, args: &Value) -> ToolResult {
    let message_id = required_string(args, "message_id")?;
    let to = required_string(args, "to")?;
    let comment = optional_string(args, "comment");
    let client = prepare_email_client(context, args).await?;
    let result = client
        .forward(message_id, to, comment)
        .await
        .map_err(tool_message)?;

    Ok(success_tool_result(
        format!(
            "forwarded message_id={} to={} status={}",
            result.message_id, to, result.status
        ),
        json!({ "forward": result }),
    ))
}

async fn call_archive_message(context: &HaiServerContext, args: &Value) -> ToolResult {
    let message_id = required_string(args, "message_id")?;
    let client = prepare_email_client(context, args).await?;
    client.archive(message_id).await.map_err(tool_message)?;

    Ok(success_tool_result(
        format!("archived message_id={message_id}"),
        json!({ "message_id": message_id, "archived": true }),
    ))
}

async fn call_unarchive_message(context: &HaiServerContext, args: &Value) -> ToolResult {
    let message_id = required_string(args, "message_id")?;
    let client = prepare_email_client(context, args).await?;
    client.unarchive(message_id).await.map_err(tool_message)?;

    Ok(success_tool_result(
        format!("unarchived message_id={message_id}"),
        json!({ "message_id": message_id, "archived": false }),
    ))
}

async fn call_list_contacts(context: &HaiServerContext, args: &Value) -> ToolResult {
    let client = prepare_email_client(context, args).await?;
    let result = client.contacts().await.map_err(tool_message)?;

    let count = result.len();
    Ok(success_tool_result(
        format!("found {count} contacts"),
        json!({ "contacts": result }),
    ))
}

async fn call_self_knowledge(args: &Value) -> ToolResult {
    let query = required_string(args, "query")?;
    let limit = optional_u32(args, "limit").unwrap_or(5) as usize;

    let results = haiai::self_knowledge::self_knowledge(query, limit);

    if results.is_empty() {
        return Ok(success_tool_result(
            "No documentation found for that query.".to_string(),
            json!({ "results": [], "count": 0 }),
        ));
    }

    let count = results.len();
    let text_summary = results
        .iter()
        .map(|r| {
            format!(
                "[{}] {} (score: {:.2})\n    Source: {}\n    {}",
                r.rank,
                r.title,
                r.score,
                r.path,
                if r.content.len() > 200 {
                    let mut end = 197;
                    while !r.content.is_char_boundary(end) {
                        end -= 1;
                    }
                    format!("{}...", &r.content[..end])
                } else {
                    r.content.clone()
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    Ok(success_tool_result(
        text_summary,
        json!({ "results": results, "count": count }),
    ))
}

// =========================================================================
// Email Template Handlers
// =========================================================================

async fn call_create_email_template(context: &HaiServerContext, args: &Value) -> ToolResult {
    let name = required_string(args, "name")?;
    let client = prepare_email_client(context, args).await?;
    let result = client
        .create_email_template(&CreateEmailTemplateOptions {
            name: name.to_string(),
            how_to_send: optional_string(args, "how_to_send").map(ToString::to_string),
            how_to_respond: optional_string(args, "how_to_respond").map(ToString::to_string),
            goal: optional_string(args, "goal").map(ToString::to_string),
            rules: optional_string(args, "rules").map(ToString::to_string),
        })
        .await
        .map_err(tool_message)?;

    Ok(success_tool_result(
        format!("created template id={} name={}", result.id, result.name),
        json!({ "template": result }),
    ))
}

async fn call_list_email_templates(context: &HaiServerContext, args: &Value) -> ToolResult {
    let client = prepare_email_client(context, args).await?;
    let result = client
        .list_email_templates(&ListEmailTemplatesOptions {
            q: optional_string(args, "q").map(ToString::to_string),
            limit: optional_u32(args, "limit"),
            offset: optional_u32(args, "offset"),
        })
        .await
        .map_err(tool_message)?;

    let count = result.templates.len();
    Ok(success_tool_result(
        format!("found {} templates (total {})", count, result.total),
        json!({ "templates": result.templates, "total": result.total }),
    ))
}

async fn call_get_email_template(context: &HaiServerContext, args: &Value) -> ToolResult {
    let template_id = required_string(args, "template_id")?;
    let client = prepare_email_client(context, args).await?;
    let result = client
        .get_email_template(template_id)
        .await
        .map_err(tool_message)?;

    Ok(success_tool_result(
        format!("template id={} name={}", result.id, result.name),
        json!({ "template": result }),
    ))
}

async fn call_update_email_template(context: &HaiServerContext, args: &Value) -> ToolResult {
    let template_id = required_string(args, "template_id")?;
    let client = prepare_email_client(context, args).await?;
    let result = client
        .update_email_template(
            template_id,
            &UpdateEmailTemplateOptions {
                name: optional_string(args, "name").map(ToString::to_string),
                how_to_send: optional_string(args, "how_to_send").map(|s| Some(s.to_string())),
                how_to_respond: optional_string(args, "how_to_respond").map(|s| Some(s.to_string())),
                goal: optional_string(args, "goal").map(|s| Some(s.to_string())),
                rules: optional_string(args, "rules").map(|s| Some(s.to_string())),
            },
        )
        .await
        .map_err(tool_message)?;

    Ok(success_tool_result(
        format!("updated template id={} name={}", result.id, result.name),
        json!({ "template": result }),
    ))
}

async fn call_delete_email_template(context: &HaiServerContext, args: &Value) -> ToolResult {
    let template_id = required_string(args, "template_id")?;
    let client = prepare_email_client(context, args).await?;
    client
        .delete_email_template(template_id)
        .await
        .map_err(tool_message)?;

    Ok(success_tool_result(
        format!("deleted template_id={template_id}"),
        json!({ "deleted": true, "template_id": template_id }),
    ))
}

fn success_tool_result(text: String, structured: Value) -> CallToolResult {
    CallToolResult {
        content: vec![Content::text(text)],
        structured_content: Some(structured),
        is_error: Some(false),
        meta: None,
    }
}

fn error_tool_result(message: String) -> CallToolResult {
    CallToolResult {
        content: vec![Content::text(message.clone())],
        structured_content: Some(json!({ "error": message })),
        is_error: Some(true),
        meta: None,
    }
}

fn required_string<'a>(args: &'a Value, key: &str) -> Result<&'a str, ToolError> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidParams(format!("{key} is required")))
}

fn optional_string<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn optional_u32(args: &Value, key: &str) -> Option<u32> {
    args.get(key)
        .and_then(Value::as_u64)
        .map(|value| value as u32)
}

fn optional_bool(args: &Value, key: &str) -> Option<bool> {
    args.get(key).and_then(Value::as_bool)
}

fn optional_string_array(args: &Value, key: &str) -> Vec<String> {
    args.get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedded_provider::EmbeddedJacsProvider;
    use haiai::NoopJacsProvider;
    use serde::Deserialize;
    use serde_json::Value;
    use std::collections::{BTreeMap, HashSet};
    use std::fs;
    use std::path::PathBuf;

    #[derive(Debug, Deserialize)]
    struct MCPToolContractFixture {
        required_tools: Vec<RequiredTool>,
    }

    #[derive(Debug, Deserialize)]
    struct RequiredTool {
        name: String,
        properties: BTreeMap<String, String>,
        required: Vec<String>,
    }

    fn load_mcp_tool_contract_fixture() -> MCPToolContractFixture {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/mcp_tool_contract.json");
        let raw = fs::read_to_string(path).expect("read mcp tool contract fixture");
        serde_json::from_str(&raw).expect("decode mcp tool contract fixture")
    }

    fn build_context() -> HaiServerContext {
        HaiServerContext::from_process_env(
            "anonymous-agent".to_string(),
            None,
            EmbeddedJacsProvider::testing("agent-123"),
        )
    }

    #[test]
    fn hai_tool_definitions_match_shared_mcp_contract() {
        let fixture = load_mcp_tool_contract_fixture();
        let actual: BTreeMap<String, (BTreeMap<String, String>, Vec<String>)> = definition_values()
            .into_iter()
            .filter_map(|tool| {
                let name = tool.get("name")?.as_str()?.to_string();
                let properties = tool
                    .get("inputSchema")?
                    .get("properties")?
                    .as_object()?
                    .iter()
                    .map(|(key, value)| {
                        let type_name = value
                            .get("type")
                            .and_then(Value::as_str)
                            .unwrap_or("string")
                            .to_string();
                        (
                            key.clone(),
                            if type_name == "integer" {
                                "number".to_string()
                            } else {
                                type_name
                            },
                        )
                    })
                    .collect();
                let mut required = tool
                    .get("inputSchema")?
                    .get("required")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(Value::as_str)
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                required.sort();
                Some((name, (properties, required)))
            })
            .collect();

        for expected in fixture.required_tools {
            let (properties, required) = actual
                .get(&expected.name)
                .unwrap_or_else(|| panic!("missing tool {}", expected.name));
            let mut expected_required = expected.required.clone();
            expected_required.sort();
            assert_eq!(required, &expected_required, "tool {}", expected.name);
            for (key, value) in &expected.properties {
                assert_eq!(
                    properties.get(key),
                    Some(value),
                    "tool {} property {}",
                    expected.name,
                    key
                );
            }
        }
    }

    #[test]
    fn fixture_contains_all_code_tools() {
        let fixture = load_mcp_tool_contract_fixture();
        let fixture_names: HashSet<String> =
            fixture.required_tools.iter().map(|t| t.name.clone()).collect();
        for tool in definition_values() {
            let name = tool["name"].as_str().expect("tool name").to_string();
            assert!(
                fixture_names.contains(&name),
                "tool '{name}' is in definition_values() but missing from mcp_tool_contract.json"
            );
        }
    }

    #[test]
    fn has_tool_matches_definitions() {
        let def_names: Vec<String> = definition_values()
            .iter()
            .filter_map(|t| t["name"].as_str().map(String::from))
            .collect();
        for name in &def_names {
            assert!(
                has_tool(name),
                "tool '{name}' is in definition_values() but not in has_tool() match"
            );
        }
    }

    #[test]
    fn fixture_total_tool_count_matches() {
        let fixture_raw: Value = serde_json::from_str(
            &fs::read_to_string(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../fixtures/mcp_tool_contract.json"),
            )
            .expect("read fixture"),
        )
        .expect("parse fixture");
        let declared = fixture_raw["total_tool_count"]
            .as_u64()
            .expect("total_tool_count field missing");
        let actual = definition_values().len() as u64;
        assert_eq!(
            declared, actual,
            "mcp_tool_contract.json total_tool_count ({declared}) != definition_values().len() ({actual})"
        );
    }

    #[test]
    fn required_string_reports_missing_fields() {
        let args = json!({});
        let err = required_string(&args, "message_id").expect_err("missing field");
        assert!(
            matches!(err, ToolError::InvalidParams(message) if message == "message_id is required")
        );
    }

    #[test]
    fn hai_self_knowledge_tool_is_recognized() {
        assert!(has_tool("hai_self_knowledge"));
    }

    #[test]
    fn hai_self_knowledge_tool_definition_exists() {
        let defs = definition_values();
        let sk_tool = defs.iter().find(|t| {
            t.get("name")
                .and_then(Value::as_str)
                .map(|s| s == "hai_self_knowledge")
                .unwrap_or(false)
        });
        assert!(
            sk_tool.is_some(),
            "hai_self_knowledge tool should be in definitions"
        );
        let schema = sk_tool.unwrap();
        let props = schema["inputSchema"]["properties"].as_object().unwrap();
        assert!(props.contains_key("query"), "should have query property");
        assert!(props.contains_key("limit"), "should have limit property");
        let required = schema["inputSchema"]["required"]
            .as_array()
            .expect("required array");
        assert!(
            required.iter().any(|v| v.as_str() == Some("query")),
            "query should be required"
        );
    }

    #[tokio::test]
    async fn hai_self_knowledge_returns_results() {
        let result = call_self_knowledge(&json!({
            "query": "JACS"
        }))
        .await
        .expect("self_knowledge result");

        assert_eq!(result.is_error, Some(false));
        let structured = result.structured_content.as_ref().expect("structured");
        let count = structured["count"].as_u64().expect("count");
        assert!(count > 0, "should return results for JACS query");
    }

    #[tokio::test]
    async fn hai_self_knowledge_empty_query_returns_no_results() {
        let result = call_self_knowledge(&json!({
            "query": ""
        }))
        .await;
        // Empty query is still valid string -- returns no results
        // (required_string will pass for empty string)
        match result {
            Ok(r) => {
                let structured = r.structured_content.as_ref().expect("structured");
                let count = structured["count"].as_u64().expect("count");
                assert_eq!(count, 0);
            }
            Err(_) => {
                // Also acceptable -- empty string could be rejected
            }
        }
    }

    #[test]
    fn hai_tool_definitions_do_not_expose_create_agent_or_runtime_hai_url_override() {
        for tool in definition_values() {
            let name = tool["name"].as_str().expect("tool name");
            assert_ne!(name, "hai_create_agent");

            let properties = tool["inputSchema"]["properties"]
                .as_object()
                .expect("tool properties");
            assert!(
                !properties.contains_key("hai_url"),
                "tool {name} should not expose runtime hai_url overrides"
            );
        }
    }

    #[test]
    fn email_agent_id_override_does_not_persist_in_cached_state() {
        let context = build_context();
        let mut client = context
            .client_with_provider(NoopJacsProvider::new("agent-123"), None)
            .expect("client");

        apply_email_identity_overrides(
            &context,
            &mut client,
            &json!({
                "agent_id": "transient-agent"
            }),
        )
        .expect("transient override should be accepted");
        assert_eq!(client.hai_agent_id(), "transient-agent");

        let mut restored = context
            .client_with_provider(NoopJacsProvider::new("agent-123"), None)
            .expect("restored client");
        context.apply_cached_agent_state("agent-123", &mut restored);

        assert_eq!(restored.hai_agent_id(), "agent-123");
        assert_eq!(restored.agent_email(), None);
    }

    #[test]
    fn email_agent_id_override_cannot_replace_cached_server_identity() {
        let context = build_context();
        context.remember_hai_agent_id("agent-123", "server-agent");

        let mut client = context
            .client_with_provider(NoopJacsProvider::new("agent-123"), None)
            .expect("client");
        context.apply_cached_agent_state("agent-123", &mut client);

        let error = apply_email_identity_overrides(
            &context,
            &mut client,
            &json!({
                "agent_id": "attacker-agent"
            }),
        )
        .expect_err("conflicting override must be rejected");

        assert!(
            matches!(error, ToolError::InvalidParams(ref message) if message.contains("agent_id")),
            "unexpected error: {error:?}"
        );
        assert_eq!(client.hai_agent_id(), "server-agent");
    }

    #[tokio::test]
    async fn call_generate_verify_link_returns_text_and_structured_content() {
        let result = call_generate_verify_link(&json!({
            "document": r#"{"signed":true}"#,
            "base_url": "https://example.com"
        }))
        .await
        .expect("verify link result");

        let url = result
            .structured_content
            .as_ref()
            .and_then(|value| value.get("verify_url"))
            .and_then(Value::as_str)
            .expect("verify_url")
            .to_string();

        assert!(url.starts_with("https://example.com/jacs/verify?s="));
        assert_eq!(
            result.content[0].as_text().map(|text| text.text.as_str()),
            Some(format!("verify_url={url}").as_str())
        );
    }
}
