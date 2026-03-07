#!/usr/bin/env python3
"""HAI SDK Quickstart -- register an agent, say hello, run a benchmark.

Prerequisites:
    pip install haiai jacs

Usage (new agent):
    export JACS_PRIVATE_KEY_PASSWORD=dev-password
    python hai_quickstart.py
or:
    export JACS_PASSWORD_FILE=/secure/path/password.txt
    python hai_quickstart.py

Usage (existing agent with jacs.config.json):
    export JACS_PRIVATE_KEY_PASSWORD=dev-password
    python hai_quickstart.py --existing
or:
    export JACS_PASSWORD_FILE=/secure/path/password.txt
    python hai_quickstart.py --existing

Configure exactly one password source.
"""

import argparse
import sys

from jacs.client import JacsClient

from haiai import config, HaiClient

HAI_URL = "https://hai.ai"
CONFIG_PATH = "./jacs.config.json"


def quickstart_new_agent():
    """Create/load JACS identity, register with HAI, and run a free benchmark."""

    # 1. Create/load local JACS identity (identity fields are required).
    print("=== Step 1: Register a new JACS agent with HAI ===")
    JacsClient.quickstart(
        name="my-quickstart-agent",
        domain="agent.example.com",
        description="HAIAI quickstart agent",
        algorithm="pq2025",
        config_path=CONFIG_PATH,
    )
    # 2. Register this identity with HAI.
    config.load(CONFIG_PATH)
    client = HaiClient()
    result = client.register(HAI_URL, owner_email="you@example.com")
    print(f"Agent registered: {result.agent_id}")

    # 3. Hello world -- verify signed connectivity
    print("\n=== Step 2: Hello world ===")
    hello = client.hello_world(HAI_URL)
    print(f"Message:   {hello.message}")
    print(f"Timestamp: {hello.timestamp}")
    print(f"Hello ID:  {hello.hello_id}")

    # 4. Check registration status
    print("\n=== Step 3: Check status ===")
    st = client.status(HAI_URL)
    print(f"Registered: {st.registered}")
    print(f"Agent ID:   {st.agent_id}")

    # 5. Run a free benchmark
    print("\n=== Step 4: Free benchmark run ===")
    run = client.free_run(HAI_URL)
    print(f"Run ID:    {run.run_id}")
    print(f"Transcript turns: {len(run.transcript)}")
    if run.upsell_message:
        print(f"Upsell: {run.upsell_message}")

    print("\nQuickstart complete!")


def quickstart_existing_agent():
    """Use an existing jacs.config.json to run hello + benchmark."""

    # 1. Load existing config
    print(
        "=== Loading existing config (requires JACS_PASSWORD_FILE or JACS_PRIVATE_KEY_PASSWORD) ==="
    )
    config.load(CONFIG_PATH)
    client = HaiClient()

    # 2. Test connection
    print("\n=== Test connection ===")
    connected = client.testconnection(HAI_URL)
    print(f"Connected: {connected}")
    if not connected:
        print("Cannot reach HAI server. Check your network.")
        sys.exit(1)

    # 3. Hello world
    print("\n=== Hello world ===")
    hello = client.hello_world(HAI_URL)
    print(f"Message:   {hello.message}")
    print(f"Hello ID:  {hello.hello_id}")

    # 4. Free benchmark
    print("\n=== Free benchmark run ===")
    run = client.free_run(HAI_URL)
    print(f"Run ID:    {run.run_id}")
    print(f"Transcript turns: {len(run.transcript)}")

    print("\nDone!")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="HAI SDK Quickstart")
    parser.add_argument(
        "--existing",
        action="store_true",
        help="Use an existing jacs.config.json instead of registering a new agent",
    )
    args = parser.parse_args()

    if args.existing:
        quickstart_existing_agent()
    else:
        quickstart_new_agent()
