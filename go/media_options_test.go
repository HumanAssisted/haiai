package haiai

import (
	"context"
	"encoding/json"
	"testing"
)

type extractionOptionsFFI struct {
	*mockFFIClient
	path string
	opts ExtractMediaSignatureOptions
}

func (m *extractionOptionsFFI) ExtractMediaSignature(path, options string) (json.RawMessage, error) {
	m.path = path
	if err := json.Unmarshal([]byte(options), &m.opts); err != nil {
		return nil, err
	}
	return json.RawMessage(`{"present":true,"payload":"extracted"}`), nil
}

func TestExtractMediaSignatureOptionsReachFFI(t *testing.T) {
	ffi := &extractionOptionsFFI{}
	client := &Client{ffi: ffi}
	result, err := client.ExtractMediaSignature(context.Background(), "stripped.png", true)
	if err != nil || !result.Present || ffi.opts.Robust || !ffi.opts.RawPayload {
		t.Fatalf("legacy extraction defaults changed: result=%v options=%+v error=%v", result, ffi.opts, err)
	}
	result, err = client.ExtractMediaSignatureWithOptions(context.Background(), "stripped.png", ExtractMediaSignatureOptions{Robust: true})
	if err != nil || !result.Present || !ffi.opts.Robust || ffi.opts.RawPayload || ffi.path != "stripped.png" {
		t.Fatalf("robust options lost: result=%v options=%+v error=%v", result, ffi.opts, err)
	}
}
