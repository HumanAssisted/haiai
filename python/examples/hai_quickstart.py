#!/usr/bin/env python3
"""HAI SDK Quickstart -- register an agent, say hello, run a benchmark.

Prerequisites:
    pip install haisdk

Usage (new agent):
    python hai_quickstart.py

Usage (existing agent with jacs.config.json):
    python hai_quickstart.py --existing
"""

import argparse
import sys

from haisdk import config, HaiClient, register_new_agent

HAI_URL = "https://hai.ai"


def quickstart_new_agent():
    """Generate a keypair, register with HAI, and run a free benchmark."""

    # 1. Register a brand-new agent (generates Ed25519 keys automatically)
    print("=== Step 1: Register a new JACS agent with HAI ===")
    result = register_new_agent(
        name="my-quickstart-agent",
        owner_email="you@example.com",
        hai_url=HAI_URL,
        key_dir="./keys",
        config_path="./jacs.config.json",
    )
    print(f"Agent registered: {result.jacs_id}")

    # 2. Create a client (config is already loaded after register_new_agent)
    client = HaiClient()

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
    print("=== Loading existing config ===")
    config.load("./jacs.config.json")
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
