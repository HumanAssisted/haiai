package haisdk

import (
	"crypto/ed25519"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"encoding/pem"
	"fmt"
	"io"
	"net/http"
	"strconv"
	"strings"
	"time"
)

const (
	maxTimestampAge    = 86400 // 24 hours
	maxTimestampFuture = 300   // 5 minutes
)

// nowFunc is the function used to get the current Unix timestamp.
// Override in tests to control timestamp validation.
var nowFunc = func() int64 { return time.Now().Unix() }

// ParseJacsSignatureHeader parses the X-JACS-Signature header into a map.
//
// Format: v=1; a=ed25519; id=agent-id; t=1740000000; s=base64sig
func ParseJacsSignatureHeader(header string) map[string]string {
	fields := make(map[string]string)
	for _, part := range strings.Split(header, ";") {
		part = strings.TrimSpace(part)
		eqIdx := strings.Index(part, "=")
		if eqIdx == -1 {
			continue
		}
		key := strings.TrimSpace(part[:eqIdx])
		value := strings.TrimSpace(part[eqIdx+1:])
		fields[key] = value
	}
	return fields
}

// VerifyEmailSignature verifies an email's JACS signature.
//
// This is a standalone function -- no agent authentication required.
// The haiURL parameter defaults to "https://hai.ai" if empty.
func VerifyEmailSignature(
	headers map[string]string,
	subject string,
	body string,
	haiURL string,
) *EmailVerificationResult {
	if haiURL == "" {
		haiURL = "https://hai.ai"
	}
	haiURL = strings.TrimRight(haiURL, "/")

	// Step 1: Extract required headers
	sigHeader := headers["X-JACS-Signature"]
	contentHashHeader := headers["X-JACS-Content-Hash"]
	fromAddress := headers["From"]

	if sigHeader == "" {
		return errResult("", "", "Missing X-JACS-Signature header")
	}
	if contentHashHeader == "" {
		return errResult("", "", "Missing X-JACS-Content-Hash header")
	}
	if strings.TrimSpace(fromAddress) == "" {
		return errResult("", "", "Missing From header")
	}

	// Step 2: Parse signature header fields
	fields := ParseJacsSignatureHeader(sigHeader)
	jacsID := fields["id"]
	timestampStr := fields["t"]
	signatureB64 := fields["s"]
	algorithm := fields["a"]
	if algorithm == "" {
		algorithm = "ed25519"
	}

	if jacsID == "" || timestampStr == "" || signatureB64 == "" {
		return errResult(jacsID, "", "Incomplete X-JACS-Signature header (missing id, t, or s)")
	}

	if algorithm != "ed25519" {
		return errResult(jacsID, "", fmt.Sprintf("Unsupported algorithm: %s", algorithm))
	}

	timestamp, err := strconv.ParseInt(timestampStr, 10, 64)
	if err != nil {
		return errResult(jacsID, "", fmt.Sprintf("Invalid timestamp: %s", timestampStr))
	}

	// Step 3: Recompute content hash
	h := sha256.Sum256([]byte(subject + "\n" + body))
	computedHash := "sha256:" + hex.EncodeToString(h[:])

	// Step 4: Compare content hashes
	if computedHash != contentHashHeader {
		return errResult(jacsID, "", "Content hash mismatch")
	}

	// Step 5: Fetch public key from registry
	registryURL := fmt.Sprintf("%s/api/agents/keys/%s", haiURL, fromAddress)
	resp, err := http.Get(registryURL) //nolint:gosec
	if err != nil {
		return errResult(jacsID, "", fmt.Sprintf("Failed to fetch public key: %v", err))
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return errResult(jacsID, "", fmt.Sprintf("Registry returned HTTP %d", resp.StatusCode))
	}

	respBody, err := io.ReadAll(resp.Body)
	if err != nil {
		return errResult(jacsID, "", fmt.Sprintf("Failed to read registry response: %v", err))
	}

	var registryData KeyRegistryResponse
	if err := json.Unmarshal(respBody, &registryData); err != nil {
		return errResult(jacsID, "", fmt.Sprintf("Failed to parse registry response: %v", err))
	}

	reputationTier := registryData.ReputationTier
	publicKeyPem := registryData.PublicKey
	registryJacsID := strings.TrimSpace(registryData.JacsID)

	if publicKeyPem == "" {
		return errResult(jacsID, reputationTier, "No public key found in registry")
	}
	if registryJacsID == "" {
		return errResult(jacsID, reputationTier, "No jacs_id found in registry")
	}
	if registryJacsID != jacsID {
		return errResult(registryJacsID, reputationTier, "Signature id does not match registry jacs_id")
	}

	// Parse PEM-encoded public key
	block, _ := pem.Decode([]byte(publicKeyPem))
	if block == nil {
		return errResult(jacsID, reputationTier, "Invalid public key PEM format")
	}

	// Extract the raw 32-byte Ed25519 public key from SPKI DER
	der := block.Bytes
	var pubKey ed25519.PublicKey
	if len(der) >= ed25519.PublicKeySize {
		pubKey = ed25519.PublicKey(der[len(der)-ed25519.PublicKeySize:])
	} else {
		return errResult(jacsID, reputationTier, fmt.Sprintf("Unsupported public key format (length %d)", len(der)))
	}

	// Step 6: Verify Ed25519 signature
	signInput := fmt.Sprintf("%s:%d", computedHash, timestamp)

	sigBytes, err := base64.StdEncoding.DecodeString(signatureB64)
	if err != nil {
		// Try URL-safe base64
		sigBytes, err = base64.RawStdEncoding.DecodeString(signatureB64)
		if err != nil {
			return errResult(jacsID, reputationTier, "Invalid signature encoding")
		}
	}

	if !ed25519.Verify(pubKey, []byte(signInput), sigBytes) {
		return errResult(registryJacsID, reputationTier, "Signature verification failed")
	}

	// Step 7: Check timestamp freshness
	now := nowFunc()
	age := now - timestamp
	if age > maxTimestampAge {
		return errResult(registryJacsID, reputationTier, "Signature timestamp is too old (>24h)")
	}
	if age < -maxTimestampFuture {
		return errResult(registryJacsID, reputationTier, "Signature timestamp is too far in the future (>5min)")
	}

	return &EmailVerificationResult{
		Valid:          true,
		JacsID:         registryJacsID,
		ReputationTier: reputationTier,
		Error:          nil,
	}
}

func errResult(jacsID, reputationTier, errMsg string) *EmailVerificationResult {
	return &EmailVerificationResult{
		Valid:          false,
		JacsID:         jacsID,
		ReputationTier: reputationTier,
		Error:          &errMsg,
	}
}
