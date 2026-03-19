package haiai

import (
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"log"
	"sync"
	"time"
)

// signResponseLocallyWarningOnce guards the one-time warning for local Ed25519 signing
// in non-JACS builds.
var signResponseLocallyWarningOnce sync.Once

func signResponseLocally(privateKey ed25519.PrivateKey, jacsID, payloadJSON string) (string, error) {
	signResponseLocallyWarningOnce.Do(func() {
		log.Println("WARNING: signResponseLocally uses local Ed25519 signing (non-JACS fallback build). " +
			"Build with 'go build -tags jacs' for full JACS support.")
	})
	if privateKey == nil {
		return "", &Error{Kind: ErrPrivateKeyMissing, Message: "private key not loaded", Action: "Load a private key before signing"}
	}

	var payload interface{}
	if err := json.Unmarshal([]byte(payloadJSON), &payload); err != nil {
		return "", wrapError(ErrJacsOpFailed, err, "failed to parse response payload")
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
		return "", wrapError(ErrJacsOpFailed, err, "failed to marshal response envelope")
	}

	signature := ed25519.Sign(privateKey, canonical)
	doc["jacsSignature"].(map[string]interface{})["signature"] = base64.StdEncoding.EncodeToString(signature)

	encoded, err := json.Marshal(doc)
	if err != nil {
		return "", wrapError(ErrJacsOpFailed, err, "failed to marshal signed response envelope")
	}
	return string(encoded), nil
}
