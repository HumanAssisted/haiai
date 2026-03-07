// Minimal HAI mediator agent in Go -- simplest possible implementation.
//
// Connects to HAI.ai via SSE, listens for benchmark jobs, and responds
// with a neutral de-escalation message each turn.
//
// Prerequisites:
//
//	Place jacs.config.json in working directory (or set JACS_CONFIG_PATH)
//	Place your Ed25519 private key as specified in jacs.config.json
//
// Usage:
//
//	go run .
package main

import (
	"context"
	"fmt"
	"log"
	"os"
	"os/signal"
	"syscall"

	haisdk "github.com/HumanAssisted/haisdk-go"
)

func main() {
	client, err := haisdk.NewClient()
	if err != nil {
		log.Fatalf("Failed to create HAI client: %v", err)
	}

	fmt.Printf("Minimal mediator connected as %s\n", client.JacsID())

	ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer cancel()

	err = client.OnBenchmarkJob(ctx, func(ctx context.Context, event haisdk.AgentEvent) error {
		conversation := event.Config.Conversation

		if len(conversation) < 2 {
			// Let the conversation start naturally
			_, err := client.SubmitResponse(ctx, event.JobID, haisdk.ModerationResponse{
				Message: "",
			})
			return err
		}

		last := conversation[len(conversation)-1]
		response := fmt.Sprintf(
			"Thank you, %s. I want to make sure both sides feel heard. "+
				"Can we take a moment to understand each other's perspective?",
			last.Speaker,
		)

		_, err := client.SubmitResponse(ctx, event.JobID, haisdk.ModerationResponse{
			Message: response,
		})

		if err == nil {
			fmt.Printf("[Job %s] Responded (turn %d)\n", event.JobID, len(conversation))
		}

		return err
	})

	if err != nil && err != context.Canceled {
		log.Fatalf("Agent error: %v", err)
	}

	fmt.Println("Agent shutting down gracefully")
}
