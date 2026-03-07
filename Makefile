.PHONY: test test-python test-node test-go test-rust

test: test-python test-node test-go test-rust

test-python:
	cd python && pip install -e ".[dev]" && pytest

test-node:
	cd node && npm ci && npm test

test-go:
	cd go && go test -race ./...

test-rust:
	cd rust && cargo test --workspace
