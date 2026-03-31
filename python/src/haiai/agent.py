"""High-level Agent API for HAI email operations.

Provides an ``Agent`` class with an ``agent.email`` namespace that wraps
the existing :class:`~haiai.client.HaiClient` for email operations.
All emails are sent signed with the agent's JACS key. There is no
unsigned send path.

Example::

    from haiai.agent import Agent

    agent = Agent.from_config("./jacs.config.json")
    result = agent.email.send(
        to="other@hai.ai",
        subject="Hello",
        body="World",
    )
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import TYPE_CHECKING, Any, Optional, Union

from haiai.client import DEFAULT_BASE_URL, HaiClient
from haiai.errors import HaiError, RateLimited
from haiai.models import (
    EmailMessage,
    EmailStatus,
    SendEmailResult,
)

if TYPE_CHECKING:
    from haiai.models import Contact


class Agent:
    """High-level agent wrapper providing ``agent.email.*`` namespace.

    Created via :meth:`Agent.from_config` which loads the JACS config
    and initializes the underlying :class:`HaiClient`. All email
    operations go through the agent's JACS key -- there is no unsigned
    path.
    """

    def __init__(self, client: HaiClient, hai_url: str) -> None:
        self._client = client
        self._hai_url = hai_url.rstrip("/")
        self.email = EmailNamespace(client, self._hai_url)

    @classmethod
    def from_config(
        cls,
        config_path: Optional[Union[str, Path]] = None,
        *,
        hai_url: str = DEFAULT_BASE_URL,
    ) -> "Agent":
        """Create an Agent from a ``jacs.config.json`` file.

        Loads the JACS agent configuration and initializes the client.
        If *config_path* is ``None``, the client uses its default
        discovery order (``JACS_CONFIG_PATH`` env var, then
        ``./jacs.config.json``).

        Args:
            config_path: Path to ``jacs.config.json``.
            hai_url: HAI API base URL (default: ``https://hai.ai``).

        Returns:
            A configured :class:`Agent` instance.
        """
        from haiai import config as hai_config

        config_str: Optional[str] = None
        if config_path is not None:
            config_str = str(config_path)
        hai_config.load(config_str)
        client = HaiClient()
        return cls(client, hai_url)

    @property
    def client(self) -> HaiClient:
        """Access the underlying :class:`HaiClient` for advanced operations."""
        return self._client


class EmailNamespace:
    """Email operations namespace.

    All methods delegate to :class:`~haiai.client.HaiClient` email
    methods. The :meth:`send` method always signs with the agent's JACS
    key via ``send_signed_email``. There is no unsigned send path.
    """

    def __init__(self, client: HaiClient, hai_url: str) -> None:
        self._client = client
        self._hai_url = hai_url

    def send(
        self,
        to: str,
        subject: str,
        body: str,
        *,
        in_reply_to: Optional[str] = None,
        attachments: Optional[list[dict[str, Any]]] = None,
        cc: Optional[list[str]] = None,
        bcc: Optional[list[str]] = None,
        labels: Optional[list[str]] = None,
    ) -> SendEmailResult:
        """Send an email, always signed with the agent's JACS key.

        Builds RFC 5322 MIME, signs with the agent's Ed25519 key via
        JACS, and submits to the HAI API. There is no unsigned send
        path.

        Args:
            to: Recipient email address.
            subject: Email subject line.
            body: Plain text email body.
            in_reply_to: Optional Message-ID for threading.
            attachments: Optional list of dicts with keys ``filename``
                (str), ``content_type`` (str), ``data`` (bytes).
            cc: Optional list of CC recipient addresses.
            bcc: Optional list of BCC recipient addresses.
            labels: Optional list of labels/tags for the message.

        Returns:
            :class:`SendEmailResult` with ``message_id`` and ``status``.

        Raises:
            HaiError: On validation or API errors.
            RateLimited: When daily send limit is exceeded.
        """
        return self._client.send_signed_email(
            hai_url=self._hai_url,
            to=to,
            subject=subject,
            body=body,
            in_reply_to=in_reply_to,
            attachments=attachments,
            cc=cc,
            bcc=bcc,
            labels=labels,
        )

    def inbox(
        self,
        *,
        limit: int = 20,
        offset: int = 0,
        is_read: Optional[bool] = None,
        folder: Optional[str] = None,
        label: Optional[str] = None,
    ) -> list[EmailMessage]:
        """List inbox messages (direction=inbound).

        Args:
            limit: Maximum number of messages to return.
            offset: Offset for pagination.
            is_read: Filter by read status (``True``/``False``/``None``).
            folder: Filter by folder (e.g. ``"inbox"``, ``"archive"``).
            label: Filter by label/tag.

        Returns:
            List of :class:`EmailMessage` objects.
        """
        return self._client.list_messages(
            hai_url=self._hai_url,
            limit=limit,
            offset=offset,
            direction="inbound",
            is_read=is_read,
            folder=folder,
            label=label,
        )

    def outbox(
        self,
        *,
        limit: int = 20,
        offset: int = 0,
        is_read: Optional[bool] = None,
        folder: Optional[str] = None,
        label: Optional[str] = None,
    ) -> list[EmailMessage]:
        """List outbox messages (direction=outbound).

        Args:
            limit: Maximum number of messages to return.
            offset: Offset for pagination.
            is_read: Filter by read status (``True``/``False``/``None``).
            folder: Filter by folder (e.g. ``"inbox"``, ``"archive"``).
            label: Filter by label/tag.

        Returns:
            List of :class:`EmailMessage` objects.
        """
        return self._client.list_messages(
            hai_url=self._hai_url,
            limit=limit,
            offset=offset,
            direction="outbound",
            is_read=is_read,
            folder=folder,
            label=label,
        )

    def get(self, message_id: str) -> EmailMessage:
        """Get a specific message by ID.

        Args:
            message_id: The message ID to retrieve.

        Returns:
            :class:`EmailMessage`.
        """
        return self._client.get_message(
            hai_url=self._hai_url,
            message_id=message_id,
        )

    def search(
        self,
        *,
        q: Optional[str] = None,
        direction: Optional[str] = None,
        from_address: Optional[str] = None,
        to_address: Optional[str] = None,
        since: Optional[str] = None,
        until: Optional[str] = None,
        is_read: Optional[bool] = None,
        jacs_verified: Optional[bool] = None,
        folder: Optional[str] = None,
        label: Optional[str] = None,
        limit: int = 20,
        offset: int = 0,
    ) -> list[EmailMessage]:
        """Search email messages.

        Args:
            q: Text search query.
            direction: Filter by direction (inbound/outbound).
            from_address: Filter by sender.
            to_address: Filter by recipient.
            since: Messages after this ISO datetime.
            until: Messages before this ISO datetime.
            is_read: Filter by read status.
            jacs_verified: Filter by JACS verification status.
            folder: Filter by folder (e.g. ``"inbox"``, ``"archive"``).
            label: Filter by label/tag.
            limit: Maximum results.
            offset: Pagination offset.

        Returns:
            List of :class:`EmailMessage` matching the search criteria.
        """
        return self._client.search_messages(
            hai_url=self._hai_url,
            q=q,
            direction=direction,
            from_address=from_address,
            to_address=to_address,
            since=since,
            until=until,
            is_read=is_read,
            jacs_verified=jacs_verified,
            folder=folder,
            label=label,
            limit=limit,
            offset=offset,
        )

    def status(self) -> EmailStatus:
        """Get email status including capacity and tier information.

        Returns:
            :class:`EmailStatus` with daily limits, usage, and tier info.
        """
        return self._client.get_email_status(hai_url=self._hai_url)

    def unread_count(self) -> int:
        """Get the count of unread messages.

        Returns:
            Number of unread inbound messages.
        """
        return self._client.get_unread_count(hai_url=self._hai_url)

    def delete(self, message_id: str) -> bool:
        """Delete a message by ID.

        Args:
            message_id: The message ID to delete.

        Returns:
            ``True`` if the message was deleted.
        """
        return self._client.delete_message(
            hai_url=self._hai_url,
            message_id=message_id,
        )

    def mark_read(self, message_id: str) -> bool:
        """Mark a message as read.

        Args:
            message_id: The message ID to mark as read.

        Returns:
            ``True`` if successful.
        """
        return self._client.mark_read(
            hai_url=self._hai_url,
            message_id=message_id,
        )

    def mark_unread(self, message_id: str) -> bool:
        """Mark a message as unread.

        Args:
            message_id: The message ID to mark as unread.

        Returns:
            ``True`` if successful.
        """
        return self._client.mark_unread(
            hai_url=self._hai_url,
            message_id=message_id,
        )

    def reply(
        self,
        message_id: str,
        body: str,
        *,
        subject: Optional[str] = None,
    ) -> SendEmailResult:
        """Reply to a message, always signed with the agent's JACS key.

        Fetches the original message, constructs a reply with proper
        threading headers, and sends it signed.

        Args:
            message_id: The message ID to reply to.
            body: Reply body text.
            subject: Optional subject override (default:
                ``Re: {original_subject}``).

        Returns:
            :class:`SendEmailResult` with ``message_id`` and ``status``.
        """
        original = self.get(message_id)
        reply_subject = subject
        if reply_subject is None:
            reply_subject = (
                original.subject
                if original.subject.startswith("Re: ")
                else f"Re: {original.subject}"
            )
        return self.send(
            to=original.from_address,
            subject=reply_subject,
            body=body,
            in_reply_to=original.message_id or original.id,
        )

    def forward(
        self,
        message_id: str,
        to: str,
        *,
        comment: Optional[str] = None,
    ) -> SendEmailResult:
        """Forward a message to another recipient.

        Args:
            message_id: The message ID to forward.
            to: Recipient email address to forward to.
            comment: Optional comment to prepend to the forwarded body.

        Returns:
            :class:`SendEmailResult` with ``message_id`` and ``status``.
        """
        return self._client.forward(
            hai_url=self._hai_url,
            message_id=message_id,
            to=to,
            comment=comment,
        )

    def archive(self, message_id: str) -> bool:
        """Archive a message.

        Args:
            message_id: The message ID to archive.

        Returns:
            ``True`` if the message was archived.
        """
        return self._client.archive(
            hai_url=self._hai_url,
            message_id=message_id,
        )

    def unarchive(self, message_id: str) -> bool:
        """Unarchive (restore) a message back to the inbox.

        Args:
            message_id: The message ID to unarchive.

        Returns:
            ``True`` if the message was unarchived.
        """
        return self._client.unarchive(
            hai_url=self._hai_url,
            message_id=message_id,
        )

    def contacts(self) -> list[Contact]:
        """List contacts derived from email history.

        Returns:
            List of :class:`Contact` objects.
        """
        return self._client.contacts(hai_url=self._hai_url)
