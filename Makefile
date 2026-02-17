.PHONY: test test-python test-node test-go

test: test-python test-node test-go

test-python:
	cd python && pip install -e ".[dev]" && pytest

test-node:
	cd node && npm ci && npm test

test-go:
	cd go && go test -race ./...
