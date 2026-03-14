use haiai::{
    generate_verify_link, generate_verify_link_hosted, CreateAgentOptions, HaiClient, JacsProvider,
    ListMessagesOptions, LocalJacsProvider, RegisterAgentOptions, SearchOptions, SendEmailOptions,
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
            | "hai_create_agent"
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
        "hai_create_agent" => call_create_agent(context, &args).await,
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
                    "username": { "type": "string" },
                    "hai_url": { "type": "string" }
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
                    "config_path": { "type": "string" },
                    "hai_url": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "hai_agent_status",
            "description": "Get the current agent's verification status",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "config_path": { "type": "string" },
                    "hai_url": { "type": "string" }
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
                    "config_path": { "type": "string" },
                    "hai_url": { "type": "string" }
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
                    "config_path": { "type": "string" },
                    "hai_url": { "type": "string" }
                },
                "required": ["agent_id", "username"]
            }
        }),
        json!({
            "name": "hai_create_agent",
            "description": "Create a new JACS agent locally and optionally register with HAI",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "password": { "type": "string" },
                    "algorithm": { "type": "string" },
                    "data_directory": { "type": "string" },
                    "key_directory": { "type": "string" },
                    "config_path": { "type": "string" },
                    "agent_type": { "type": "string" },
                    "description": { "type": "string" },
                    "domain": { "type": "string" },
                    "default_storage": { "type": "string" },
                    "register_with_hai": { "type": "boolean" },
                    "owner_email": { "type": "string" },
                    "hai_url": { "type": "string" }
                },
                "required": ["name", "password"]
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
                    "description": { "type": "string" },
                    "hai_url": { "type": "string" }
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
            "description": "Send an email from the agent's @hai.ai address",
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
                    "email": { "type": "string", "description": "Optional claimed @hai.ai sender address for stateless MCP sessions" },
                    "config_path": { "type": "string" },
                    "hai_url": { "type": "string" }
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
                    "config_path": { "type": "string" },
                    "hai_url": { "type": "string" }
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
                    "config_path": { "type": "string" },
                    "hai_url": { "type": "string" }
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
                    "config_path": { "type": "string" },
                    "hai_url": { "type": "string" }
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
                    "config_path": { "type": "string" },
                    "hai_url": { "type": "string" }
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
                    "config_path": { "type": "string" },
                    "hai_url": { "type": "string" }
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
                    "config_path": { "type": "string" },
                    "hai_url": { "type": "string" }
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
                    "config_path": { "type": "string" },
                    "hai_url": { "type": "string" }
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
                    "config_path": { "type": "string" },
                    "hai_url": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "hai_reply_email",
            "description": "Reply to an email message (fetches original, sends reply with threading)",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "message_id": { "type": "string", "description": "ID of the message to reply to" },
                    "body": { "type": "string", "description": "Reply body text" },
                    "subject_override": { "type": "string", "description": "Override the Re: subject line" },
                    "agent_id": { "type": "string", "description": "Optional HAI agent UUID for stateless MCP sessions" },
                    "email": { "type": "string", "description": "Optional claimed @hai.ai sender address for stateless MCP sessions" },
                    "config_path": { "type": "string" },
                    "hai_url": { "type": "string" }
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
                    "email": { "type": "string", "description": "Optional claimed @hai.ai sender address for stateless MCP sessions" },
                    "config_path": { "type": "string" },
                    "hai_url": { "type": "string" }
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
                    "config_path": { "type": "string" },
                    "hai_url": { "type": "string" }
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
                    "config_path": { "type": "string" },
                    "hai_url": { "type": "string" }
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
                    "config_path": { "type": "string" },
                    "hai_url": { "type": "string" }
                }
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

async fn call_create_agent(context: &HaiServerContext, args: &Value) -> ToolResult {
    let options = CreateAgentOptions {
        name: required_string(args, "name")?.to_string(),
        password: required_string(args, "password")?.to_string(),
        algorithm: optional_string(args, "algorithm").map(ToString::to_string),
        data_directory: optional_string(args, "data_directory").map(ToString::to_string),
        key_directory: optional_string(args, "key_directory").map(ToString::to_string),
        config_path: optional_string(args, "config_path").map(ToString::to_string),
        agent_type: optional_string(args, "agent_type").map(ToString::to_string),
        description: optional_string(args, "description").map(ToString::to_string),
        domain: optional_string(args, "domain").map(ToString::to_string),
        default_storage: optional_string(args, "default_storage").map(ToString::to_string),
    };

    let created = LocalJacsProvider::create_agent_with_options(&options).map_err(tool_message)?;

    let register_with_hai = args
        .get("register_with_hai")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let registration = if register_with_hai {
        let config_path = (!created.config_path.is_empty()).then_some(created.config_path.as_str());
        let provider = context.local_provider(config_path).map_err(tool_message)?;
        let created_jacs_id = provider.jacs_id().to_string();
        let agent_json = provider.export_agent_json().map_err(tool_message)?;
        let public_key_pem = provider.public_key_pem().map_err(tool_message)?;

        let client = context
            .client_with_provider(provider, optional_string(args, "hai_url"))
            .map_err(tool_message)?;
        let register_result = client
            .register(&RegisterAgentOptions {
                agent_json,
                public_key_pem: Some(public_key_pem),
                owner_email: optional_string(args, "owner_email").map(ToString::to_string),
                domain: optional_string(args, "domain").map(ToString::to_string),
                description: optional_string(args, "description").map(ToString::to_string),
            })
            .await
            .map_err(tool_message)?;
        context.remember_hai_agent_id(&created_jacs_id, &register_result.agent_id);

        Some(register_result)
    } else {
        None
    };

    Ok(success_tool_result(
        if registration.is_some() {
            format!(
                "created agent_id={} and registered with HAI",
                created.agent_id
            )
        } else {
            format!("created agent_id={}", created.agent_id)
        },
        json!({
            "created_agent": created,
            "registration": registration
        }),
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
) {
    let jacs_id = client.jacs_id().to_string();

    if let Some(agent_id) = optional_string(args, "agent_id").filter(|value| !value.is_empty()) {
        client.set_hai_agent_id(agent_id.to_string());
        context.remember_hai_agent_id(&jacs_id, agent_id);
    }

    if let Some(email) = optional_string(args, "email").filter(|value| !value.is_empty()) {
        client.set_agent_email(email.to_string());
        context.remember_agent_email(&jacs_id, email);
    }
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
    apply_email_identity_overrides(context, &mut client, args);

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
        .send_email(&SendEmailOptions {
            to: required_string(args, "to")?.to_string(),
            subject: required_string(args, "subject")?.to_string(),
            body: required_string(args, "body")?.to_string(),
            cc: optional_string_array(args, "cc"),
            bcc: optional_string_array(args, "bcc"),
            in_reply_to: optional_string(args, "in_reply_to").map(ToString::to_string),
            attachments: vec![],
            labels: optional_string_array(args, "labels"),
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
    use serde::Deserialize;
    use serde_json::Value;
    use std::collections::BTreeMap;
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
    fn required_string_reports_missing_fields() {
        let args = json!({});
        let err = required_string(&args, "message_id").expect_err("missing field");
        assert!(
            matches!(err, ToolError::InvalidParams(message) if message == "message_id is required")
        );
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
