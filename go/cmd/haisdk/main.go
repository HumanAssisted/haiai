// Command haisdk provides a CLI for HAI agent management.
//
// Usage:
//
//	haisdk register --name "My Agent" --description "..." --dns "agent.example" --owner-email "user@example.com"
//	haisdk hello
//	haisdk benchmark --tier free
//	haisdk verify --jacs-id <id>
//	haisdk check-username --username <name>
//	haisdk claim-username --username <name> --agent-id <id>
//	haisdk send-email --to <addr> --subject <subj> --body <body>
//	haisdk list-messages
package main

import (
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"path/filepath"

	haisdk "github.com/HumanAssisted/haisdk-go"
)

func main() {
	if len(os.Args) < 2 {
		printUsage()
		os.Exit(1)
	}

	cmd := os.Args[1]

	switch cmd {
	case "register":
		cmdRegister(os.Args[2:])
	case "hello":
		cmdHello(os.Args[2:])
	case "benchmark":
		cmdBenchmark(os.Args[2:])
	case "verify":
		cmdVerify(os.Args[2:])
	case "check-username":
		cmdCheckUsername(os.Args[2:])
	case "claim-username":
		cmdClaimUsername(os.Args[2:])
	case "send-email":
		cmdSendEmail(os.Args[2:])
	case "list-messages":
		cmdListMessages(os.Args[2:])
	case "--help", "-h", "help":
		printUsage()
	default:
		fmt.Fprintf(os.Stderr, "unknown command: %s\n", cmd)
		printUsage()
		os.Exit(1)
	}
}

func printUsage() {
	fmt.Println(`haisdk - HAI SDK CLI for agent management

Commands:
  register        Register a new JACS agent
  hello           Run hello handshake
  benchmark       Run a benchmark
  verify          Check agent verification status
  check-username  Check username availability
  claim-username  Claim a username
  send-email      Send an agent email
  list-messages   List inbox messages
Global environment variables:
  HAI_URL            API base URL (default: https://api.hai.ai)
  JACS_CONFIG_PATH   Path to jacs.config.json

Use "haisdk <command> --help" for more information.`)
}

func newClient() *haisdk.Client {
	cl, err := haisdk.NewClient()
	if err != nil {
		fmt.Fprintf(os.Stderr, "error: %v\n", err)
		os.Exit(1)
	}
	return cl
}

func printJSON(v interface{}) {
	data, _ := json.MarshalIndent(v, "", "  ")
	fmt.Println(string(data))
}

func fatal(msg string, err error) {
	fmt.Fprintf(os.Stderr, "%s: %v\n", msg, err)
	os.Exit(1)
}

func defaultSecureKeyDir() string {
	home, err := os.UserHomeDir()
	if err != nil || home == "" {
		return ".keys"
	}
	return filepath.Join(home, ".jacs", "keys")
}

func cmdRegister(args []string) {
	fs := flag.NewFlagSet("register", flag.ExitOnError)
	name := fs.String("name", "", "Agent name (required)")
	description := fs.String("description", "", "Agent description (required)")
	dns := fs.String("dns", "", "Agent DNS domain (required)")
	domainAlias := fs.String("domain", "", "Alias for --dns")
	email := fs.String("owner-email", "", "Owner email (required)")
	keyDir := fs.String("key-dir", "", "Key directory (default: ~/.jacs/keys)")
	configPath := fs.String("config-path", "jacs.config.json", "Config file path")
	fs.Parse(args)

	domain := *dns
	if domain == "" {
		domain = *domainAlias
	}

	if *name == "" || *description == "" || domain == "" || *email == "" {
		fmt.Fprintln(os.Stderr, "error: --name, --description, --dns, and --owner-email are required")
		fs.Usage()
		os.Exit(1)
	}
	if *keyDir == "" {
		*keyDir = defaultSecureKeyDir()
	}
	if absKeyDir, err := filepath.Abs(*keyDir); err == nil {
		*keyDir = absKeyDir
	}
	if absConfigPath, err := filepath.Abs(*configPath); err == nil {
		*configPath = absConfigPath
	}

	opts := &haisdk.RegisterNewAgentOptions{
		Description: *description,
		OwnerEmail:  *email,
		Domain:      domain,
	}
	endpoint := os.Getenv("HAI_URL")
	if endpoint == "" {
		endpoint = haisdk.DefaultEndpoint
	}
	result, err := haisdk.RegisterNewAgentWithEndpoint(context.Background(), endpoint, *name, opts)
	if err != nil {
		fatal("registration failed", err)
	}
	if result.Registration == nil {
		fatal("registration failed", fmt.Errorf("empty registration response"))
	}

	if err := os.MkdirAll(*keyDir, 0o700); err != nil {
		fatal("failed to create key directory", err)
	}
	_ = os.Chmod(*keyDir, 0o700)

	privateKeyPath := filepath.Join(*keyDir, "agent_private_key.pem")
	publicKeyPath := filepath.Join(*keyDir, "agent_public_key.pem")
	if err := os.WriteFile(privateKeyPath, result.PrivateKey, 0o600); err != nil {
		fatal("failed to write private key", err)
	}
	if err := os.WriteFile(publicKeyPath, result.PublicKey, 0o644); err != nil {
		fatal("failed to write public key", err)
	}

	cfg := map[string]string{
		"jacsAgentName":    *name,
		"jacsAgentVersion": "1.0.0",
		"jacsKeyDir":       *keyDir,
		"jacsId":           result.Registration.JacsID,
	}
	cfgJSON, err := json.MarshalIndent(cfg, "", "  ")
	if err != nil {
		fatal("failed to encode config", err)
	}
	if err := os.MkdirAll(filepath.Dir(*configPath), 0o700); err != nil {
		fatal("failed to create config directory", err)
	}
	if err := os.WriteFile(*configPath, append(cfgJSON, '\n'), 0o600); err != nil {
		fatal("failed to write config", err)
	}

	printJSON(result.Registration)
	fmt.Printf("\nConfig saved to %s\n", *configPath)
	fmt.Printf("Keys saved to %s\n", *keyDir)
	fmt.Printf("Check %s for verification link.\n", *email)
}

func cmdHello(args []string) {
	fs := flag.NewFlagSet("hello", flag.ExitOnError)
	fs.Parse(args)

	cl := newClient()
	result, err := cl.Hello(context.Background())
	if err != nil {
		fatal("hello failed", err)
	}
	printJSON(result)
}

func cmdBenchmark(args []string) {
	fs := flag.NewFlagSet("benchmark", flag.ExitOnError)
	tier := fs.String("tier", "free", "Benchmark tier (free, dns_certified, fully_certified)")
	fs.Parse(args)

	cl := newClient()
	result, err := cl.Benchmark(context.Background(), *tier)
	if err != nil {
		fatal("benchmark failed", err)
	}
	printJSON(result)
}

func cmdVerify(args []string) {
	fs := flag.NewFlagSet("verify", flag.ExitOnError)
	jacsID := fs.String("jacs-id", "", "JACS ID to verify (required)")
	fs.Parse(args)

	if *jacsID == "" {
		fmt.Fprintln(os.Stderr, "error: --jacs-id is required")
		fs.Usage()
		os.Exit(1)
	}

	cl := newClient()
	result, err := cl.VerifyAgent(context.Background(), *jacsID)
	if err != nil {
		fatal("verify failed", err)
	}
	printJSON(result)
}

func cmdCheckUsername(args []string) {
	fs := flag.NewFlagSet("check-username", flag.ExitOnError)
	username := fs.String("username", "", "Username to check (required)")
	fs.Parse(args)

	if *username == "" {
		fmt.Fprintln(os.Stderr, "error: --username is required")
		fs.Usage()
		os.Exit(1)
	}

	cl := newClient()
	result, err := cl.CheckUsername(context.Background(), *username)
	if err != nil {
		fatal("check-username failed", err)
	}
	printJSON(result)
}

func cmdClaimUsername(args []string) {
	fs := flag.NewFlagSet("claim-username", flag.ExitOnError)
	username := fs.String("username", "", "Username to claim (required)")
	agentID := fs.String("agent-id", "", "Agent ID (required)")
	fs.Parse(args)

	if *username == "" || *agentID == "" {
		fmt.Fprintln(os.Stderr, "error: --username and --agent-id are required")
		fs.Usage()
		os.Exit(1)
	}

	cl := newClient()
	result, err := cl.ClaimUsername(context.Background(), *agentID, *username)
	if err != nil {
		fatal("claim-username failed", err)
	}
	printJSON(result)
}

func cmdSendEmail(args []string) {
	fs := flag.NewFlagSet("send-email", flag.ExitOnError)
	to := fs.String("to", "", "Recipient address (required)")
	subject := fs.String("subject", "", "Email subject (required)")
	body := fs.String("body", "", "Email body (required)")
	inReplyTo := fs.String("in-reply-to", "", "Message ID to reply to (optional)")
	fs.Parse(args)

	if *to == "" || *subject == "" || *body == "" {
		fmt.Fprintln(os.Stderr, "error: --to, --subject, and --body are required")
		fs.Usage()
		os.Exit(1)
	}

	cl := newClient()
	result, err := cl.SendEmailWithOptions(context.Background(), haisdk.SendEmailOptions{
		To:        *to,
		Subject:   *subject,
		Body:      *body,
		InReplyTo: *inReplyTo,
	})
	if err != nil {
		fatal("send-email failed", err)
	}
	printJSON(result)
}

func cmdListMessages(args []string) {
	fs := flag.NewFlagSet("list-messages", flag.ExitOnError)
	limit := fs.Int("limit", 20, "Maximum messages to return")
	offset := fs.Int("offset", 0, "Messages to skip")
	folder := fs.String("folder", "inbox", "Folder: inbox, outbox, all")
	fs.Parse(args)

	cl := newClient()
	messages, err := cl.ListMessages(context.Background(), haisdk.ListMessagesOptions{
		Limit:  *limit,
		Offset: *offset,
		Folder: *folder,
	})
	if err != nil {
		fatal("list-messages failed", err)
	}
	printJSON(messages)
}
