.PHONY: test test-python test-node test-go test-rust \
        smoke smoke-python smoke-node smoke-go \
        build-python-ffi build-node-ffi \
        versions check-versions check-jacs-versions \
        bump-version bump-jacs-version \
        generate-knowledge check-knowledge \
        release-node release-python release-rust release-all \
        release-delete-tags \
        retry-rust retry-python retry-node retry-everything \
        help

# ============================================================================
# VERSION DETECTION
# ============================================================================

RUST_VERSION := $(shell grep '^version' rust/haiai/Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
CLI_VERSION := $(shell grep '^version' rust/haiai-cli/Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
MCP_VERSION := $(shell grep '^version' rust/hai-mcp/Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
BINDING_CORE_VERSION := $(shell grep '^version' rust/hai-binding-core/Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
HAIINPM_VERSION := $(shell grep '^version' rust/haiinpm/Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
HAIIPY_VERSION := $(shell grep '^version' rust/haiipy/Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
HAIIGO_VERSION := $(shell grep '^version' rust/haiigo/Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
PYTHON_VERSION := $(shell grep '^version' python/pyproject.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
NODE_VERSION := $(shell grep '"version"' node/package.json | head -1 | sed 's/.*: *"\(.*\)".*/\1/')
PLUGIN_VERSION := $(shell grep '"version"' .claude-plugin/plugin.json | head -1 | sed 's/.*: *"\(.*\)".*/\1/')

# JACS dependency versions across SDKs
JACS_RUST := $(shell grep '^jacs ' rust/haiai/Cargo.toml | sed 's/.*"\(=*[0-9][^"]*\)".*/\1/' | sed 's/^=//')
JACS_RUST_CLI := $(shell grep '^jacs ' rust/haiai-cli/Cargo.toml | sed 's/.*"\(=*[0-9][^"]*\)".*/\1/' | sed 's/^=//')
JACS_RUST_MCP := $(shell grep '^jacs ' rust/hai-mcp/Cargo.toml | sed 's/.*"\(=*[0-9][^"]*\)".*/\1/' | sed 's/^=//')
JACS_PYTHON := $(shell grep 'jacs==' python/pyproject.toml | sed 's/.*jacs==\([^"]*\)".*/\1/')
JACS_NODE := $(shell grep '@hai.ai/jacs' node/package.json | head -1 | sed 's/.*: *"\(.*\)".*/\1/')

# ============================================================================
# TEST
# ============================================================================

test: test-python test-node test-go test-rust

test-python: build-python-ffi
	cd python && pip install -e ".[dev,mcp]" && pytest

test-node:
	cd node && npm ci && npm test

test-go:
	cd go && CGO_ENABLED=1 go test -race ./...

test-rust:
	cd rust && cargo test --workspace

# ============================================================================
# SMOKE — real-FFI smoke tests (skip cleanly when native artifacts missing)
# ============================================================================
#
# Each smoke test loads the real native binding (haiipy / haiinpm / libhaiigo)
# and round-trips `save_memory("smoke")` against a real local HTTP listener.
# This is the one test category that catches FFI-method-shape regressions
# (i.e. surface declared but not implemented in the native layer).
#
# Smoke tests are SKIPPED when the native binding isn't built — they don't
# fail. To run them, build the native artifacts first:
#   make build-python-ffi build-node-ffi
#   cargo build -p haiigo --release   # produces rust/target/release/libhaiigo.dylib

smoke: smoke-python smoke-node smoke-go

smoke-python:
	cd python && pytest -m native_smoke -v

smoke-node:
	cd node && npm test -- --run ffi-native-smoke

smoke-go:
	cd go && CGO_ENABLED=1 \
	    CGO_LDFLAGS="-L$(CURDIR)/rust/target/release" \
	    DYLD_LIBRARY_PATH="$(CURDIR)/rust/target/release" \
	    go test -tags cgo_smoke -run NativeSmoke -v ./...

# ============================================================================
# BUILD (local FFI wheel builds for development)
# ============================================================================

build-python-ffi:
	cd rust/haiipy && pip install maturin && maturin develop --release

build-node-ffi:
	cd rust/haiinpm && npm install && npm run build

# ============================================================================
# KNOWLEDGE (self-knowledge document embedding)
# ============================================================================

generate-knowledge:
	./scripts/generate_knowledge.sh

check-knowledge:
	./scripts/check_knowledge_freshness.sh

# ============================================================================
# VERSION INFO
# ============================================================================

versions:
	@echo "Detected versions:"
	@echo "  rust/haiai             $(RUST_VERSION)"
	@echo "  rust/haiai-cli         $(CLI_VERSION)"
	@echo "  rust/hai-mcp           $(MCP_VERSION)"
	@echo "  rust/hai-binding-core  $(BINDING_CORE_VERSION)"
	@echo "  rust/haiinpm           $(HAIINPM_VERSION)"
	@echo "  rust/haiipy            $(HAIIPY_VERSION)"
	@echo "  rust/haiigo            $(HAIIGO_VERSION)"
	@echo "  python                 $(PYTHON_VERSION)"
	@echo "  node                   $(NODE_VERSION)"
	@echo "  plugin                 $(PLUGIN_VERSION)"
	@echo ""
	@if [ "$(RUST_VERSION)" = "$(CLI_VERSION)" ] && \
		[ "$(RUST_VERSION)" = "$(MCP_VERSION)" ] && \
		[ "$(RUST_VERSION)" = "$(BINDING_CORE_VERSION)" ] && \
		[ "$(RUST_VERSION)" = "$(HAIINPM_VERSION)" ] && \
		[ "$(RUST_VERSION)" = "$(HAIIPY_VERSION)" ] && \
		[ "$(RUST_VERSION)" = "$(HAIIGO_VERSION)" ] && \
		[ "$(RUST_VERSION)" = "$(PYTHON_VERSION)" ] && \
		[ "$(RUST_VERSION)" = "$(NODE_VERSION)" ] && \
		[ "$(RUST_VERSION)" = "$(PLUGIN_VERSION)" ]; then \
		echo "All versions match: $(RUST_VERSION)"; \
	else \
		echo "WARNING: Versions do not match!"; \
	fi

version: versions

check-versions:
	@if [ "$(RUST_VERSION)" != "$(CLI_VERSION)" ]; then \
		echo "ERROR: haiai ($(RUST_VERSION)) != haiai-cli ($(CLI_VERSION))"; exit 1; fi
	@if [ "$(RUST_VERSION)" != "$(MCP_VERSION)" ]; then \
		echo "ERROR: haiai ($(RUST_VERSION)) != hai-mcp ($(MCP_VERSION))"; exit 1; fi
	@if [ "$(RUST_VERSION)" != "$(BINDING_CORE_VERSION)" ]; then \
		echo "ERROR: haiai ($(RUST_VERSION)) != hai-binding-core ($(BINDING_CORE_VERSION))"; exit 1; fi
	@if [ "$(RUST_VERSION)" != "$(HAIINPM_VERSION)" ]; then \
		echo "ERROR: haiai ($(RUST_VERSION)) != haiinpm ($(HAIINPM_VERSION))"; exit 1; fi
	@if [ "$(RUST_VERSION)" != "$(HAIIPY_VERSION)" ]; then \
		echo "ERROR: haiai ($(RUST_VERSION)) != haiipy ($(HAIIPY_VERSION))"; exit 1; fi
	@if [ "$(RUST_VERSION)" != "$(HAIIGO_VERSION)" ]; then \
		echo "ERROR: haiai ($(RUST_VERSION)) != haiigo ($(HAIIGO_VERSION))"; exit 1; fi
	@if [ "$(RUST_VERSION)" != "$(PYTHON_VERSION)" ]; then \
		echo "ERROR: haiai ($(RUST_VERSION)) != python ($(PYTHON_VERSION))"; exit 1; fi
	@if [ "$(RUST_VERSION)" != "$(NODE_VERSION)" ]; then \
		echo "ERROR: haiai ($(RUST_VERSION)) != node ($(NODE_VERSION))"; exit 1; fi
	@if [ "$(RUST_VERSION)" != "$(PLUGIN_VERSION)" ]; then \
		echo "ERROR: haiai ($(RUST_VERSION)) != plugin ($(PLUGIN_VERSION))"; exit 1; fi
	@echo "All versions match: $(RUST_VERSION)"

bump-version:
	@if [ -z "$(V)" ]; then echo "Usage: make bump-version V=major|minor|patch"; exit 1; fi
	./scripts/bump-version.sh $(V)

bump-jacs-version:
	@if [ -z "$(V)" ]; then echo "Usage: make bump-jacs-version V=0.9.10"; exit 1; fi
	./scripts/bump-jacs-version.sh $(V)

check-jacs-versions:
	@echo "JACS dependency versions:"
	@echo "  rust/haiai      $(JACS_RUST)"
	@echo "  rust/haiai-cli  $(JACS_RUST_CLI)"
	@echo "  rust/hai-mcp    $(JACS_RUST_MCP)"
	@echo "  python          $(JACS_PYTHON)"
	@if [ "$(JACS_RUST)" != "$(JACS_RUST_CLI)" ]; then \
		echo "ERROR: jacs in haiai ($(JACS_RUST)) != haiai-cli ($(JACS_RUST_CLI))"; exit 1; fi
	@if [ "$(JACS_RUST)" != "$(JACS_RUST_MCP)" ]; then \
		echo "ERROR: jacs in haiai ($(JACS_RUST)) != hai-mcp ($(JACS_RUST_MCP))"; exit 1; fi
	@if [ "$(JACS_RUST)" != "$(JACS_PYTHON)" ]; then \
		echo "ERROR: jacs in haiai ($(JACS_RUST)) != python ($(JACS_PYTHON))"; exit 1; fi
	@case "$(JACS_NODE)" in \
		file:*) echo "  node            $(JACS_NODE) (local path, skipping match check)" ;; \
		*) if [ "$(JACS_RUST)" != "$(JACS_NODE)" ]; then \
			echo "ERROR: jacs in haiai ($(JACS_RUST)) != node ($(JACS_NODE))"; exit 1; fi ;; \
	esac
	@echo "All JACS versions match: $(JACS_RUST)"

# ============================================================================
# GITHUB CI RELEASE (via git tags)
# ============================================================================
# Create git tags that trigger GitHub Actions release workflows.
# Versions are auto-detected from source files.
# Safe to re-run — existing tags are deleted and recreated.
#
# Required GitHub Secrets:
#   - CRATES_IO_TOKEN  (for rust/v* tags)
#   - PYPI_API_TOKEN   (for python/v* tags, or use trusted publisher)
#   - NPM_TOKEN        (for node/v* tags)
# ============================================================================

release-rust:
	@echo "Releasing Rust v$(RUST_VERSION)..."
	-git tag -d rust/v$(RUST_VERSION) 2>/dev/null
	-git push origin --delete rust/v$(RUST_VERSION) 2>/dev/null
	git tag rust/v$(RUST_VERSION)
	git push origin rust/v$(RUST_VERSION)
	@echo "Tagged rust/v$(RUST_VERSION) - CI will publish to crates.io + build CLI binaries"

release-python:
	@echo "Releasing Python v$(PYTHON_VERSION)..."
	-git tag -d python/v$(PYTHON_VERSION) 2>/dev/null
	-git push origin --delete python/v$(PYTHON_VERSION) 2>/dev/null
	git tag python/v$(PYTHON_VERSION)
	git push origin python/v$(PYTHON_VERSION)
	@echo "Tagged python/v$(PYTHON_VERSION) - CI will publish to PyPI"

release-node:
	@echo "Releasing Node v$(NODE_VERSION)..."
	-git tag -d node/v$(NODE_VERSION) 2>/dev/null
	-git push origin --delete node/v$(NODE_VERSION) 2>/dev/null
	git tag node/v$(NODE_VERSION)
	git push origin node/v$(NODE_VERSION)
	@echo "Tagged node/v$(NODE_VERSION) - CI will publish to npm"

release-all: check-versions check-jacs-versions release-rust
	@echo "Waiting 30s for Rust CI to start building CLI binaries..."
	@sleep 30
	$(MAKE) release-python release-node
	@echo "All release tags pushed for v$(RUST_VERSION). CI will handle publishing."
	@echo "Note: Node and Python CI will retry up to 6 min waiting for Rust binaries."

release-delete-tags:
	@echo "Deleting tags for version $(RUST_VERSION)..."
	-git tag -d rust/v$(RUST_VERSION) python/v$(PYTHON_VERSION) node/v$(NODE_VERSION)
	-git push origin --delete rust/v$(RUST_VERSION) python/v$(PYTHON_VERSION) node/v$(NODE_VERSION)
	@echo "Deleted release tags"

# ============================================================================
# RETRY FAILED RELEASES
# ============================================================================
# Safe retry: delete old tags (local+remote), retag, push to re-trigger CI.

retry-rust:
	@echo "Retrying Rust release for v$(RUST_VERSION)..."
	-git tag -d rust/v$(RUST_VERSION)
	-git push origin --delete rust/v$(RUST_VERSION)
	git tag rust/v$(RUST_VERSION)
	git push origin rust/v$(RUST_VERSION)
	@echo "✓ Re-tagged rust/v$(RUST_VERSION) - CI will retry crates.io + CLI binaries"

retry-python:
	@echo "Retrying Python release for v$(PYTHON_VERSION)..."
	-git tag -d python/v$(PYTHON_VERSION)
	-git push origin --delete python/v$(PYTHON_VERSION)
	git tag python/v$(PYTHON_VERSION)
	git push origin python/v$(PYTHON_VERSION)
	@echo "✓ Re-tagged python/v$(PYTHON_VERSION) - CI will retry PyPI publish"

retry-node:
	@echo "Retrying Node release for v$(NODE_VERSION)..."
	-git tag -d node/v$(NODE_VERSION)
	-git push origin --delete node/v$(NODE_VERSION)
	git tag node/v$(NODE_VERSION)
	git push origin node/v$(NODE_VERSION)
	@echo "✓ Re-tagged node/v$(NODE_VERSION) - CI will retry npm publish"

# Smart retry: check each registry and only retry releases that haven't published yet.
retry-everything:
	@echo "Checking which releases need retrying for v$(RUST_VERSION)..."
	@echo ""
	@NEED_RETRY=""; \
	if curl -sf "https://crates.io/api/v1/crates/haiai/$(RUST_VERSION)" > /dev/null 2>&1; then \
		echo "  crates.io  haiai $(RUST_VERSION) — already published, skipping"; \
	else \
		echo "  crates.io  haiai $(RUST_VERSION) — NOT found, will retry"; \
		NEED_RETRY="$$NEED_RETRY rust"; \
	fi; \
	if curl -sf "https://pypi.org/pypi/haiai/$(PYTHON_VERSION)/json" > /dev/null 2>&1; then \
		echo "  PyPI       haiai $(PYTHON_VERSION) — already published, skipping"; \
	else \
		echo "  PyPI       haiai $(PYTHON_VERSION) — NOT found, will retry"; \
		NEED_RETRY="$$NEED_RETRY python"; \
	fi; \
	if npm view "@haiai/haiai@$(NODE_VERSION)" version > /dev/null 2>&1; then \
		echo "  npm        @haiai/haiai $(NODE_VERSION) — already published, skipping"; \
	else \
		echo "  npm        @haiai/haiai $(NODE_VERSION) — NOT found, will retry"; \
		NEED_RETRY="$$NEED_RETRY node"; \
	fi; \
	echo ""; \
	if [ -z "$$NEED_RETRY" ]; then \
		echo "✓ All releases already published for v$(RUST_VERSION). Nothing to retry."; \
	else \
		echo "Retrying:$$NEED_RETRY"; \
		echo ""; \
		for target in $$NEED_RETRY; do \
			case $$target in \
				rust) \
					echo "--- Retrying crates.io + CLI ---"; \
					git tag -d rust/v$(RUST_VERSION) 2>/dev/null || true; \
					git push origin --delete rust/v$(RUST_VERSION) 2>/dev/null || true; \
					git tag rust/v$(RUST_VERSION); \
					git push origin rust/v$(RUST_VERSION); \
					echo "✓ Re-tagged rust/v$(RUST_VERSION)"; \
					;; \
				python) \
					echo "--- Retrying PyPI ---"; \
					git tag -d python/v$(PYTHON_VERSION) 2>/dev/null || true; \
					git push origin --delete python/v$(PYTHON_VERSION) 2>/dev/null || true; \
					git tag python/v$(PYTHON_VERSION); \
					git push origin python/v$(PYTHON_VERSION); \
					echo "✓ Re-tagged python/v$(PYTHON_VERSION)"; \
					;; \
				node) \
					echo "--- Retrying npm ---"; \
					git tag -d node/v$(NODE_VERSION) 2>/dev/null || true; \
					git push origin --delete node/v$(NODE_VERSION) 2>/dev/null || true; \
					git tag node/v$(NODE_VERSION); \
					git push origin node/v$(NODE_VERSION); \
					echo "✓ Re-tagged node/v$(NODE_VERSION)"; \
					;; \
			esac; \
		done; \
		echo ""; \
		echo "✓ Retry tags pushed. GitHub CI will handle publishing."; \
	fi

# ============================================================================
# HELP
# ============================================================================

help:
	@echo "HAIAI SDK Makefile"
	@echo ""
	@echo "VERSION INFO:"
	@echo "  make versions        Show all detected versions"
	@echo "  make check-versions       Verify all package versions match"
	@echo "  make check-jacs-versions  Verify JACS dep versions match across SDKs"
	@echo "  make bump-version V=patch       Bump SDK version (major|minor|patch)"
	@echo "  make bump-jacs-version V=0.9.10 Bump JACS dep version across all SDKs"
	@echo ""
	@echo "TEST:"
	@echo "  make test            Run all tests"
	@echo "  make test-python     Run Python tests"
	@echo "  make test-node       Run Node tests"
	@echo "  make test-go         Run Go tests"
	@echo "  make test-rust       Run Rust tests"
	@echo ""
	@echo "KNOWLEDGE:"
	@echo "  make generate-knowledge  Regenerate self-knowledge docs (requires ../JACS)"
	@echo "  make check-knowledge     Fail if self_knowledge_data.rs is stale"
	@echo ""
	@echo "RELEASE (via git tags, versions auto-detected, safe to re-run):"
	@echo "  make release-rust    Tag rust/v<ver>   -> crates.io + CLI binaries"
	@echo "  make release-python  Tag python/v<ver> -> PyPI"
	@echo "  make release-node    Tag node/v<ver>   -> npm"
	@echo "  make release-all     Verify versions, then release all"
	@echo "  make release-delete-tags  Delete all tags for current version"
	@echo ""
	@echo "RETRY (safe retry for failed releases — delete tags, retag, push):"
	@echo "  make retry-rust      Retry crates.io + CLI release"
	@echo "  make retry-python    Retry PyPI release"
	@echo "  make retry-node      Retry npm release"
	@echo "  make retry-everything  Smart retry: check registries, only retry unpublished"
	@echo ""
	@echo "Required GitHub Secrets:"
	@echo "  CRATES_IO_TOKEN  - for rust/v* tags"
	@echo "  PYPI_API_TOKEN   - for python/v* tags (or trusted publisher)"
	@echo "  NPM_TOKEN        - for node/v* tags"
