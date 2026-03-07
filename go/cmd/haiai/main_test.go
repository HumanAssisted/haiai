package main

import (
	"bytes"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

func TestDefaultSecureKeyDirUsesHome(t *testing.T) {
	t.Setenv("HOME", "/tmp/haiai-home")

	got := defaultSecureKeyDir()
	want := filepath.Join("/tmp/haiai-home", ".jacs", "keys")
	if got != want {
		t.Fatalf("defaultSecureKeyDir() = %q, want %q", got, want)
	}
}

func TestEncryptPrivateKeyPEMRejectsInvalidPEM(t *testing.T) {
	_, err := encryptPrivateKeyPEM([]byte("not-a-pem"), []byte("secret"))
	if err == nil || !strings.Contains(err.Error(), "invalid private key PEM") {
		t.Fatalf("encryptPrivateKeyPEM() error = %v, want invalid private key PEM", err)
	}
}

func TestMainNoArgsPrintsUsageAndExitsOne(t *testing.T) {
	stdout, stderr, exitCode := runCLI(t)
	if exitCode != 1 {
		t.Fatalf("exitCode = %d, want 1", exitCode)
	}
	if !strings.Contains(stdout, "haiai - HAI SDK CLI for agent management") {
		t.Fatalf("stdout = %q, want usage banner", stdout)
	}
	if stderr != "" && !strings.Contains(stderr, "fallback crypto") {
		t.Fatalf("stderr = %q, want only optional startup warning", stderr)
	}
}

func TestMainHelpPrintsUsageAndExitsZero(t *testing.T) {
	stdout, _, exitCode := runCLI(t, "--help")
	if exitCode != 0 {
		t.Fatalf("exitCode = %d, want 0", exitCode)
	}
	if !strings.Contains(stdout, `Use "haiai <command> --help" for more information.`) {
		t.Fatalf("stdout = %q, want help footer", stdout)
	}
}

func TestMainUnknownCommandPrintsErrorAndUsage(t *testing.T) {
	stdout, stderr, exitCode := runCLI(t, "unknown-command")
	if exitCode != 1 {
		t.Fatalf("exitCode = %d, want 1", exitCode)
	}
	if !strings.Contains(stderr, "unknown command: unknown-command") {
		t.Fatalf("stderr = %q, want unknown command error", stderr)
	}
	if !strings.Contains(stdout, "Commands:") {
		t.Fatalf("stdout = %q, want usage output", stdout)
	}
}

func TestVerifyRequiresJacsID(t *testing.T) {
	stdout, stderr, exitCode := runCLI(t, "verify")
	if exitCode != 1 {
		t.Fatalf("exitCode = %d, want 1", exitCode)
	}
	if !strings.Contains(stderr, "error: --jacs-id is required") {
		t.Fatalf("stderr = %q, want missing jacs-id error", stderr)
	}
	if stdout != "" {
		t.Fatalf("stdout = %q, want empty", stdout)
	}
	if !strings.Contains(stderr, "-jacs-id string") {
		t.Fatalf("stderr = %q, want verify flag usage", stderr)
	}
}

func TestSendEmailRequiresAllFields(t *testing.T) {
	stdout, stderr, exitCode := runCLI(t, "send-email", "--to", "ops@hai.ai")
	if exitCode != 1 {
		t.Fatalf("exitCode = %d, want 1", exitCode)
	}
	if !strings.Contains(stderr, "error: --to, --subject, and --body are required") {
		t.Fatalf("stderr = %q, want missing send-email flags error", stderr)
	}
	if stdout != "" {
		t.Fatalf("stdout = %q, want empty", stdout)
	}
	if !strings.Contains(stderr, "-subject string") {
		t.Fatalf("stderr = %q, want send-email usage", stderr)
	}
}

func runCLI(t *testing.T, args ...string) (stdout string, stderr string, exitCode int) {
	t.Helper()

	cmdArgs := append([]string{"-test.run=TestCLIHelperProcess", "--"}, args...)
	cmd := exec.Command(os.Args[0], cmdArgs...)
	cmd.Env = append(os.Environ(), "GO_WANT_HELPER_PROCESS=1")

	var stdoutBuf bytes.Buffer
	var stderrBuf bytes.Buffer
	cmd.Stdout = &stdoutBuf
	cmd.Stderr = &stderrBuf

	err := cmd.Run()
	if err == nil {
		return stdoutBuf.String(), stderrBuf.String(), 0
	}

	var exitErr *exec.ExitError
	if !errors.As(err, &exitErr) {
		t.Fatalf("runCLI() unexpected error: %v", err)
	}
	return stdoutBuf.String(), stderrBuf.String(), exitErr.ExitCode()
}

func TestCLIHelperProcess(t *testing.T) {
	if os.Getenv("GO_WANT_HELPER_PROCESS") != "1" {
		return
	}

	dashIdx := -1
	for i, arg := range os.Args {
		if arg == "--" {
			dashIdx = i
			break
		}
	}

	if dashIdx == -1 {
		os.Exit(2)
	}

	os.Args = append([]string{"haiai"}, os.Args[dashIdx+1:]...)
	main()
	os.Exit(0)
}
