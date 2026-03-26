// HAI SDK Quickstart (Go) -- register an agent, say hello, run a benchmark.
//
// Prerequisites:
//
//	go get github.com/HumanAssisted/haiai-go
//
// Recommended usage (JACS-managed identity):
//
//	export JACS_PRIVATE_KEY_PASSWORD=dev-password
//	jacs quickstart --algorithm pq2025
//	go run . --existing
//
// or:
//
//	export JACS_PASSWORD_FILE=/secure/path/password.txt
//	jacs quickstart --algorithm pq2025
//	go run . --existing
//
// Legacy bootstrap usage (local keygen path, prefer `jacs quickstart` instead):
//
//	export JACS_PRIVATE_KEY_PASSWORD=dev-password
//	go run . --bootstrap-local
//
// or:
//
//	export JACS_PASSWORD_FILE=/secure/path/password.txt
//	go run . --bootstrap-local
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

	haiai "github.com/HumanAssisted/haiai-go"
)

const HAIURL = haiai.DefaultEndpoint

func main() {
	existing := flag.Bool("existing", true, "Use existing jacs.config.json/JACS_CONFIG_PATH (recommended)")
	bootstrapLocal := flag.Bool("bootstrap-local", false, "Use legacy local bootstrap registration path")
	flag.Parse()

	if *bootstrapLocal {
		quickstartNew()
		return
	}
	if *existing {
		quickstartExisting()
		return
	}
	quickstartExisting()
}

func quickstartNew() {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Minute)
	defer cancel()

	// 1. Register a new agent without requiring a pre-existing local config.
	fmt.Println("=== Step 1: Register a new JACS agent with HAI ===")
	reg, err := haiai.RegisterNewAgentWithEndpoint(
		ctx,
		HAIURL,
		"my-quickstart-agent",
		&haiai.RegisterNewAgentOptions{
			OwnerEmail: "you@example.com",
		},
	)
	if err != nil {
		log.Fatalf("Registration failed: %v", err)
	}
	if !reg.Success {
		log.Fatal("Registration failed: server returned success=false")
	}
	fmt.Printf("Agent ID: %s\n", reg.AgentID)

	jacsID := reg.JacsID
	if jacsID == "" {
		jacsID = reg.AgentID
	}

	// 2. Create an authenticated client from the registered agent config.
	// The FFI layer wrote a jacs.config.json during registration.
	client, err := haiai.NewClient(
		haiai.WithEndpoint(HAIURL),
		haiai.WithJACSID(jacsID),
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

	// 1. Load existing JACS-managed config
	fmt.Println("=== Loading existing config (configure exactly one password source) ===")
	client, err := haiai.NewClient(haiai.WithEndpoint(HAIURL))
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
