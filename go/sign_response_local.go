package haiai

import (
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"time"
)

func signResponseLocally(privateKey ed25519.PrivateKey, jacsID, payloadJSON string) (string, error) {
	if privateKey == nil {
		return "", fmt.Errorf("private key not loaded")
	}

	var payload interface{}
	if err := json.Unmarshal([]byte(payloadJSON), &payload); err != nil {
		return "", fmt.Errorf("failed to parse response payload: %w", err)
	}

	doc := map[string]interface{}{
		"jacsId":      jacsID,
		"jacsVersion": "1.0.0",
		"jacsSignature": map[string]interface{}{
			"agentID": jacsID,
			"date":    time.Now().UTC().Format(time.RFC3339),
		},
		"response": payload,
	}

	canonical, err := json.Marshal(doc)
	if err != nil {
		return "", fmt.Errorf("failed to marshal response envelope: %w", err)
	}

	signature := ed25519.Sign(privateKey, canonical)
	doc["jacsSignature"].(map[string]interface{})["signature"] = base64.StdEncoding.EncodeToString(signature)

	encoded, err := json.Marshal(doc)
	if err != nil {
		return "", fmt.Errorf("failed to marshal signed response envelope: %w", err)
	}
	return string(encoded), nil
}
