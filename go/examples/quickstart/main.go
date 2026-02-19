// HAI SDK Quickstart (Go) -- register an agent, say hello, run a benchmark.
//
// Prerequisites:
//
//	go get github.com/HumanAssisted/haisdk-go
//
// Usage (new agent):
//
//	export JACS_PRIVATE_KEY_PASSWORD=dev-password
//	go run .
// or:
//	export JACS_PASSWORD_FILE=/secure/path/password.txt
//	go run .
//
// Usage (existing agent with jacs.config.json):
//
//	export JACS_PRIVATE_KEY_PASSWORD=dev-password
//	go run . --existing
// or:
//	export JACS_PASSWORD_FILE=/secure/path/password.txt
//	go run . --existing
//
// Configure exactly one password source.
package main

import (
	"context"
	"flag"
	"fmt"
	"log"
	"os"
	"time"

	haisdk "github.com/HumanAssisted/haisdk-go"
)

const HAIURL = haisdk.DefaultEndpoint

func main() {
	existing := flag.Bool("existing", false, "Use existing jacs.config.json")
	flag.Parse()

	if *existing {
		quickstartExisting()
	} else {
		quickstartNew()
	}
}

func quickstartNew() {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Minute)
	defer cancel()

	// 1. Register a new agent without requiring a pre-existing local config.
	fmt.Println("=== Step 1: Register a new JACS agent with HAI ===")
	reg, err := haisdk.RegisterNewAgentWithEndpoint(
		ctx,
		HAIURL,
		"my-quickstart-agent",
		&haisdk.RegisterNewAgentOptions{
			OwnerEmail: "you@example.com",
		},
	)
	if err != nil {
		log.Fatalf("Registration failed: %v", err)
	}
	if reg.Registration == nil {
		log.Fatal("Registration failed: empty registration response")
	}
	fmt.Printf("Agent ID: %s\n", reg.Registration.AgentID)

	privateKey, err := haisdk.ParsePrivateKey(reg.PrivateKey)
	if err != nil {
		log.Fatalf("Failed to parse generated private key: %v", err)
	}
	jacsID := reg.Registration.JacsID
	if jacsID == "" {
		jacsID = reg.Registration.AgentID
	}

	// 2. Create an authenticated client from the in-memory bootstrap credentials.
	client, err := haisdk.NewClient(
		haisdk.WithEndpoint(HAIURL),
		haisdk.WithJACSID(jacsID),
		haisdk.WithPrivateKey(privateKey),
	)
	if err != nil {
		log.Fatalf("Failed to create client from bootstrap credentials: %v", err)
	}

	// 3. Hello world -- verify signed connectivity
	fmt.Println("\n=== Step 2: Hello world ===")
	hello, err := client.Hello(ctx)
	if err != nil {
		log.Fatalf("Hello failed: %v", err)
	}
	fmt.Printf("Message:   %s\n", hello.Message)
	fmt.Printf("Timestamp: %s\n", hello.Timestamp)
	fmt.Printf("Hello ID:  %s\n", hello.HelloID)

	// 4. Check registration status
	fmt.Println("\n=== Step 3: Check status ===")
	status, err := client.Status(ctx)
	if err != nil {
		log.Fatalf("Status check failed: %v", err)
	}
	fmt.Printf("Registered: %v\n", status.Registered)
	fmt.Printf("JACS ID:    %s\n", status.JacsID)

	// 5. Run a free benchmark
	fmt.Println("\n=== Step 4: Free benchmark run ===")
	run, err := client.FreeRun(ctx)
	if err != nil {
		log.Fatalf("Benchmark failed: %v", err)
	}
	fmt.Printf("Run ID: %s\n", run.RunID)
	fmt.Printf("Score:  %.2f\n", run.Score)

	fmt.Println("\nQuickstart complete!")
}

func quickstartExisting() {
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Minute)
	defer cancel()

	// 1. Load existing config
	fmt.Println("=== Loading existing config (configure exactly one password source) ===")
	client, err := haisdk.NewClient(haisdk.WithEndpoint(HAIURL))
	if err != nil {
		log.Fatalf("Failed to create client: %v", err)
	}

	// 2. Test connection
	fmt.Println("\n=== Test connection ===")
	connected, err := client.TestConnection(ctx)
	if err != nil {
		log.Fatalf("Connection test failed: %v", err)
	}
	fmt.Printf("Connected: %v\n", connected)
	if !connected {
		fmt.Fprintln(os.Stderr, "Cannot reach HAI server. Check your network.")
		os.Exit(1)
	}

	// 3. Hello world
	fmt.Println("\n=== Hello world ===")
	hello, err := client.Hello(ctx)
	if err != nil {
		log.Fatalf("Hello failed: %v", err)
	}
	fmt.Printf("Message:   %s\n", hello.Message)
	fmt.Printf("Hello ID:  %s\n", hello.HelloID)

	// 4. Free benchmark
	fmt.Println("\n=== Free benchmark run ===")
	run, err := client.FreeRun(ctx)
	if err != nil {
		log.Fatalf("Benchmark failed: %v", err)
	}
	fmt.Printf("Run ID: %s\n", run.RunID)
	fmt.Printf("Score:  %.2f\n", run.Score)

	fmt.Println("\nDone!")
}
