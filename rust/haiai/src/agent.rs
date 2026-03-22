//! High-level `Agent` API for HAI email operations.
//!
//! Provides an ergonomic `Agent` struct with an `agent.email` namespace
//! that wraps [`HaiClient`] and ensures all emails are signed with the
//! agent's JACS key. There is no unsigned send path.
//!
//! # Example
//!
//! ```rust,no_run
//! use haiai::agent::Agent;
//! use haiai::types::SendEmailOptions;
//!
//! # async fn example() -> haiai::Result<()> {
//! let agent = Agent::from_config(None).await?;
//! agent.email.send(SendEmailOptions {
//!     to: "other@hai.ai".into(),
//!     subject: "Hello".into(),
//!     body: "World".into(),
//!     cc: vec![],
//!     bcc: vec![],
//!     in_reply_to: None,
//!     attachments: vec![],
//!     labels: vec![],
//! }).await?;
//! # Ok(())
//! # }
//! ```

use std::path::Path;
use std::sync::Arc;

use crate::client::{HaiClient, HaiClientOptions};
use crate::error::Result;
#[cfg(feature = "jacs-crate")]
use crate::jacs_local::LocalJacsProvider;
use crate::types::{
    Contact, EmailMessage, EmailStatus, ListMessagesOptions, SearchOptions, SendEmailOptions,
    SendEmailResult,
};
use crate::validation;

/// High-level agent wrapper providing `agent.email.*` namespace.
///
/// Created via [`Agent::from_config`] which loads the JACS config and
/// initializes the signing provider. All email operations go through
/// the agent's JACS key -- there is no unsigned path.
pub struct Agent {
    /// Email operations namespace.
    pub email: EmailNamespace,
}

#[cfg(feature = "jacs-crate")]
impl Agent {
    /// Create an Agent from a `jacs.config.json` file.
    ///
    /// Loads the JACS agent configuration, initializes the local signing
    /// provider, and creates the email namespace. If `config_path` is None,
    /// looks for `JACS_CONFIG_PATH` env var or `./jacs.config.json`.
    ///
    /// # Arguments
    /// * `config_path` - Path to jacs.config.json (None for default discovery)
    pub async fn from_config(config_path: Option<&Path>) -> Result<Self> {
        Self::from_config_with_options(config_path, HaiClientOptions::default()).await
    }

    /// Create an Agent with custom client options.
    ///
    /// # Arguments
    /// * `config_path` - Path to jacs.config.json (None for default discovery)
    /// * `options` - Client options (base URL, timeout, retries)
    pub async fn from_config_with_options(
        config_path: Option<&Path>,
        options: HaiClientOptions,
    ) -> Result<Self> {
        let provider = LocalJacsProvider::from_config_path(config_path, None)?;
        let client = HaiClient::new(provider, options)?;
        let client = Arc::new(tokio::sync::RwLock::new(client));

        Ok(Self {
            email: EmailNamespace {
                client: Arc::clone(&client),
            },
        })
    }

    /// Get the underlying client for advanced operations.
    pub fn client(&self) -> &Arc<tokio::sync::RwLock<HaiClient<LocalJacsProvider>>> {
        &self.email.client
    }
}

/// Email operations namespace.
///
/// All methods delegate to [`HaiClient`] email methods. The `send` method
/// always signs with the agent's JACS key via `send_signed_email`. There
/// is no unsigned send path.
pub struct EmailNamespace {
    #[cfg(feature = "jacs-crate")]
    client: Arc<tokio::sync::RwLock<HaiClient<LocalJacsProvider>>>,
}

#[cfg(feature = "jacs-crate")]
impl EmailNamespace {
    /// Send an email, always signed with the agent's JACS key.
    ///
    /// Builds RFC 5322 MIME, signs with the agent's Ed25519 key via JACS,
    /// and submits to the HAI API. There is no unsigned send path.
    ///
    /// # Arguments
    /// * `options` - Email options (to, subject, body, attachments, etc.)
    ///
    /// # Errors
    /// Returns `HaiError::Validation` if input validation fails (CRLF
    /// injection, invalid email address, oversized attachments).
    pub async fn send(&self, options: SendEmailOptions) -> Result<SendEmailResult> {
        validation::validate_send_email(&options)?;
        let client = self.client.read().await;
        client.send_signed_email(&options).await
    }

    /// List inbox messages (direction=inbound).
    ///
    /// # Arguments
    /// * `options` - List options (limit, offset)
    pub async fn inbox(&self, options: ListMessagesOptions) -> Result<Vec<EmailMessage>> {
        let mut opts = options;
        opts.direction = Some("inbound".to_string());
        let client = self.client.read().await;
        client.list_messages(&opts).await
    }

    /// List outbox messages (direction=outbound).
    ///
    /// # Arguments
    /// * `options` - List options (limit, offset)
    pub async fn outbox(&self, options: ListMessagesOptions) -> Result<Vec<EmailMessage>> {
        let mut opts = options;
        opts.direction = Some("outbound".to_string());
        let client = self.client.read().await;
        client.list_messages(&opts).await
    }

    /// Get a specific message by ID.
    ///
    /// # Arguments
    /// * `message_id` - The message ID to retrieve
    pub async fn get(&self, message_id: &str) -> Result<EmailMessage> {
        let client = self.client.read().await;
        client.get_message(message_id).await
    }

    /// Search messages.
    ///
    /// # Arguments
    /// * `options` - Search options (query, direction, date range, etc.)
    pub async fn search(&self, options: SearchOptions) -> Result<Vec<EmailMessage>> {
        let client = self.client.read().await;
        client.search_messages(&options).await
    }

    /// Get email status including capacity and tier information.
    pub async fn status(&self) -> Result<EmailStatus> {
        let client = self.client.read().await;
        client.get_email_status().await
    }

    /// Get the count of unread messages.
    pub async fn unread_count(&self) -> Result<u64> {
        let client = self.client.read().await;
        client.get_unread_count().await
    }

    /// Delete a message by ID.
    ///
    /// # Arguments
    /// * `message_id` - The message ID to delete
    pub async fn delete(&self, message_id: &str) -> Result<()> {
        let client = self.client.read().await;
        client.delete_message(message_id).await
    }

    /// Mark a message as read.
    ///
    /// # Arguments
    /// * `message_id` - The message ID to mark as read
    pub async fn mark_read(&self, message_id: &str) -> Result<()> {
        let client = self.client.read().await;
        client.mark_read(message_id).await
    }

    /// Mark a message as unread.
    ///
    /// # Arguments
    /// * `message_id` - The message ID to mark as unread
    pub async fn mark_unread(&self, message_id: &str) -> Result<()> {
        let client = self.client.read().await;
        client.mark_unread(message_id).await
    }

    /// Reply to a message, always signed with the agent's JACS key.
    ///
    /// Fetches the original message, constructs a reply with proper
    /// threading headers, and sends it signed.
    ///
    /// # Arguments
    /// * `message_id` - The message ID to reply to
    /// * `body` - Reply body text
    /// * `subject_override` - Optional subject override (default: "Re: {original_subject}")
    pub async fn reply(
        &self,
        message_id: &str,
        body: &str,
        subject_override: Option<&str>,
    ) -> Result<SendEmailResult> {
        let client = self.client.read().await;
        client.reply(message_id, body, subject_override).await
    }

    /// Forward a message to another recipient.
    ///
    /// # Arguments
    /// * `message_id` - The message ID to forward
    /// * `to` - Recipient email address
    /// * `comment` - Optional comment to prepend to the forwarded message
    pub async fn forward(
        &self,
        message_id: &str,
        to: &str,
        comment: Option<&str>,
    ) -> Result<SendEmailResult> {
        let client = self.client.read().await;
        client.forward(message_id, to, comment).await
    }

    /// Archive a message.
    ///
    /// # Arguments
    /// * `message_id` - The message ID to archive
    pub async fn archive(&self, message_id: &str) -> Result<()> {
        let client = self.client.read().await;
        client.archive(message_id).await
    }

    /// Unarchive a message.
    ///
    /// # Arguments
    /// * `message_id` - The message ID to unarchive
    pub async fn unarchive(&self, message_id: &str) -> Result<()> {
        let client = self.client.read().await;
        client.unarchive(message_id).await
    }

    /// List email contacts.
    pub async fn contacts(&self) -> Result<Vec<Contact>> {
        let client = self.client.read().await;
        client.contacts().await
    }
}
