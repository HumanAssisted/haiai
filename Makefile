VERSION := 0.1.1

.PHONY: test test-python test-node test-go test-rust \
        tag untag-old publish-node publish-python publish-rust publish-all

test: test-python test-node test-go test-rust

test-python:
	cd python && pip install -e ".[dev]" && pytest

test-node:
	cd node && npm ci && npm test

test-go:
	cd go && go test -race ./...

test-rust:
	cd rust && cargo test --workspace

# ── Release tagging ──────────────────────────────────────────────

tag:
	git tag v$(VERSION)
	git push origin v$(VERSION)

untag-old:
	git tag -d v0.9.0 || true
	git push origin :refs/tags/v0.9.0 || true

publish-node:
	git tag node/v$(VERSION)
	git push origin node/v$(VERSION)

publish-python:
	git tag python/v$(VERSION)
	git push origin python/v$(VERSION)

publish-rust:
	git tag rust/v$(VERSION)
	git push origin rust/v$(VERSION)

publish-all: tag publish-node publish-python publish-rust
