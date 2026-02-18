"""HAI SDK command-line interface.

Usage::

    python -m jacs.hai register --name "My Agent" --owner-email "user@example.com"
    python -m jacs.hai hello --api-url https://hai.ai
    python -m jacs.hai benchmark --tier free
    haisdk --help
"""

from __future__ import annotations

import argparse
import json
import sys
from typing import Optional, Sequence

DEFAULT_API_URL = "https://hai.ai"


def _load_config_if_exists() -> None:
    """Attempt to load jacs.config.json from the default location."""
    from jacs.hai.config import is_loaded, load

    if is_loaded():
        return
    try:
        load()
    except (FileNotFoundError, ValueError):
        pass


def _require_config() -> None:
    """Load config or exit with a helpful error."""
    _load_config_if_exists()
    from jacs.hai.config import is_loaded

    if not is_loaded():
        print(
            "Error: No JACS config found. Run 'haisdk register' first or "
            "set JACS_CONFIG_PATH.",
            file=sys.stderr,
        )
        sys.exit(1)


def cmd_register(args: argparse.Namespace) -> None:
    """Register a new agent."""
    from jacs.hai.client import register_new_agent

    result = register_new_agent(
        name=args.name,
        owner_email=args.owner_email,
        version=args.version,
        hai_url=args.api_url,
        key_dir=args.key_dir,
        config_path=args.config_path,
        domain=args.dns,
        description=args.description,
    )
    print(f"Agent registered successfully!")
    print(f"  JACS ID:  {result.jacs_id}")
    print(f"  Agent ID: {result.agent_id}")
    print(f"\nNext steps:")
    print(f"  1. Check {args.owner_email} for a verification email")
    print(f"  2. Run: haisdk hello")
    print(f"  3. Run: haisdk benchmark --tier free")


def cmd_hello(args: argparse.Namespace) -> None:
    """Run hello world handshake."""
    _require_config()
    from jacs.hai.client import hello_world

    result = hello_world(args.api_url, include_test=args.include_test)
    print(f"Hello from HAI!")
    print(f"  Message:   {result.message}")
    print(f"  Timestamp: {result.timestamp}")
    print(f"  Client IP: {result.client_ip}")
    if result.hai_signature_valid:
        print(f"  HAI signature: valid")


def cmd_benchmark(args: argparse.Namespace) -> None:
    """Run a benchmark."""
    _require_config()
    from jacs.hai.client import HaiClient

    client = HaiClient()
    result = client.benchmark(args.api_url, tier=args.tier, name=args.name)
    print(f"Benchmark complete!")
    print(f"  Score:  {result.score}")
    print(f"  Passed: {result.passed}/{result.total}")


def cmd_verify(args: argparse.Namespace) -> None:
    """Check agent verification status."""
    _require_config()
    from jacs.hai.client import HaiClient

    client = HaiClient()
    result = client.status(args.api_url)
    print(f"Agent: {result.agent_id}")
    print(f"  Registered: {result.registered}")
    if result.registered_at:
        print(f"  Since: {result.registered_at}")
    if result.hai_signatures:
        print(f"  Algorithms: {', '.join(result.hai_signatures)}")


def cmd_check_username(args: argparse.Namespace) -> None:
    """Check username availability."""
    from jacs.hai.client import HaiClient

    client = HaiClient()
    result = client.check_username(args.api_url, args.username)
    available = result.get("available", False)
    if available:
        print(f"'{args.username}@hai.ai' is available!")
    else:
        reason = result.get("reason", "already taken")
        print(f"'{args.username}@hai.ai' is not available: {reason}")


def cmd_claim_username(args: argparse.Namespace) -> None:
    """Claim a username."""
    _require_config()
    from jacs.hai.client import HaiClient
    from jacs.hai.config import get_config

    client = HaiClient()
    agent_id = args.agent_id or get_config().jacs_id
    if not agent_id:
        print("Error: --agent-id required (or load a config with jacsId)", file=sys.stderr)
        sys.exit(1)
    result = client.claim_username(args.api_url, agent_id, args.username)
    print(f"Username claimed: {result.get('email', args.username + '@hai.ai')}")


def cmd_send_email(args: argparse.Namespace) -> None:
    """Send an email."""
    _require_config()
    from jacs.hai.client import HaiClient

    client = HaiClient()
    result = client.send_email(
        args.api_url, args.to, args.subject, args.body, args.in_reply_to,
    )
    print(f"Email sent! Message ID: {result.message_id}")


def cmd_list_messages(args: argparse.Namespace) -> None:
    """List inbox messages."""
    _require_config()
    from jacs.hai.client import HaiClient

    client = HaiClient()
    messages = client.list_messages(
        args.api_url, limit=args.limit, folder=args.folder,
    )
    if not messages:
        print("No messages.")
        return
    for msg in messages:
        read_marker = " " if msg.read_at else "*"
        print(f"  {read_marker} [{msg.id[:8]}] {msg.from_address} -> {msg.subject}")


def cmd_fetch_key(args: argparse.Namespace) -> None:
    """Fetch a remote agent's public key."""
    from jacs.hai.client import HaiClient

    client = HaiClient()
    info = client.fetch_remote_key(args.api_url, args.jacs_id, args.version)
    print(f"Agent:     {info.jacs_id}")
    print(f"Algorithm: {info.algorithm}")
    print(f"Status:    {info.status}")
    print(f"DNS:       {'verified' if info.dns_verified else 'not verified'}")
    if args.show_key:
        print(f"\n{info.public_key}")


def build_parser() -> argparse.ArgumentParser:
    """Build the argument parser."""
    parser = argparse.ArgumentParser(
        prog="haisdk",
        description="HAI SDK CLI -- register, test, and manage AI agents",
    )
    parser.add_argument(
        "--api-url",
        default=DEFAULT_API_URL,
        help=f"HAI API URL (default: {DEFAULT_API_URL})",
    )

    sub = parser.add_subparsers(dest="command", help="Available commands")

    # register
    p = sub.add_parser("register", help="Register a new JACS agent")
    p.add_argument("--name", required=True, help="Agent display name")
    p.add_argument("--description", required=True, help="Agent description")
    p.add_argument("--dns", "--domain", dest="dns", required=True, help="DNS domain for verification")
    p.add_argument("--owner-email", required=True, help="Owner's email address")
    p.add_argument("--version", default="1.0.0", help="Agent version (default: 1.0.0)")
    p.add_argument("--key-dir", default=None, help="Directory for key files (default: ~/.jacs/keys)")
    p.add_argument("--config-path", default="./jacs.config.json", help="Config file path")

    # hello
    p = sub.add_parser("hello", help="Run hello world handshake")
    p.add_argument("--include-test", action="store_true", help="Include test scenario")

    # benchmark
    p = sub.add_parser("benchmark", help="Run a benchmark")
    p.add_argument("--tier", default="free", choices=["free", "dns_certified", "fully_certified"])
    p.add_argument("--name", default="mediator", help="Benchmark scenario name")

    # verify
    sub.add_parser("verify", help="Check agent registration status")

    # check-username
    p = sub.add_parser("check-username", help="Check @hai.ai username availability")
    p.add_argument("--username", required=True, help="Username to check")

    # claim-username
    p = sub.add_parser("claim-username", help="Claim a @hai.ai username")
    p.add_argument("--username", required=True, help="Username to claim")
    p.add_argument("--agent-id", default=None, help="Agent ID (default: from config)")

    # send-email
    p = sub.add_parser("send-email", help="Send an email from agent's @hai.ai address")
    p.add_argument("--to", required=True, help="Recipient address")
    p.add_argument("--subject", required=True, help="Email subject")
    p.add_argument("--body", required=True, help="Email body")
    p.add_argument("--in-reply-to", default=None, help="Message ID to reply to")

    # list-messages
    p = sub.add_parser("list-messages", help="List inbox messages")
    p.add_argument("--limit", type=int, default=20, help="Max messages to show")
    p.add_argument("--folder", default="inbox", choices=["inbox", "sent"])

    # fetch-key
    p = sub.add_parser("fetch-key", help="Fetch a remote agent's public key")
    p.add_argument("--jacs-id", required=True, help="Target agent's JACS ID")
    p.add_argument("--version", default="latest", help="Key version (default: latest)")
    p.add_argument("--show-key", action="store_true", help="Print the full PEM key")

    return parser


COMMANDS = {
    "register": cmd_register,
    "hello": cmd_hello,
    "benchmark": cmd_benchmark,
    "verify": cmd_verify,
    "check-username": cmd_check_username,
    "claim-username": cmd_claim_username,
    "send-email": cmd_send_email,
    "list-messages": cmd_list_messages,
    "fetch-key": cmd_fetch_key,
}


def main(argv: Optional[Sequence[str]] = None) -> None:
    """CLI entry point."""
    parser = build_parser()
    args = parser.parse_args(argv)

    if not args.command:
        parser.print_help()
        sys.exit(0)

    handler = COMMANDS.get(args.command)
    if handler is None:
        parser.print_help()
        sys.exit(1)

    try:
        handler(args)
    except KeyboardInterrupt:
        print("\nAborted.", file=sys.stderr)
        sys.exit(130)
    except SystemExit:
        raise
    except Exception as exc:
        print(f"Error: {exc}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
