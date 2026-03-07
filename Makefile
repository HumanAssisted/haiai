.PHONY: test test-python test-node test-go test-rust \
        versions check-versions \
        release-node release-python release-rust release-all \
        release-delete-tags retry-node retry-python retry-rust \
        check-version-node check-version-python check-version-rust \
        help

# ============================================================================
# VERSION DETECTION
# ============================================================================

RUST_VERSION := $(shell grep '^version' rust/haiai/Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
CLI_VERSION := $(shell grep '^version' rust/haiai-cli/Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
MCP_VERSION := $(shell grep '^version' rust/hai-mcp/Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
PYTHON_VERSION := $(shell grep '^version' python/pyproject.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
NODE_VERSION := $(shell grep '"version"' node/package.json | head -1 | sed 's/.*: *"\(.*\)".*/\1/')

# ============================================================================
# TEST
# ============================================================================

test: test-python test-node test-go test-rust

test-python:
	cd python && pip install -e ".[dev]" && pytest

test-node:
	cd node && npm ci && npm test

test-go:
	cd go && go test -race ./...

test-rust:
	cd rust && cargo test --workspace

# ============================================================================
# VERSION INFO
# ============================================================================

versions:
	@echo "Detected versions:"
	@echo "  rust/haiai      $(RUST_VERSION)"
	@echo "  rust/haiai-cli  $(CLI_VERSION)"
	@echo "  rust/hai-mcp    $(MCP_VERSION)"
	@echo "  python          $(PYTHON_VERSION)"
	@echo "  node            $(NODE_VERSION)"
	@echo ""
	@if [ "$(RUST_VERSION)" = "$(CLI_VERSION)" ] && \
		[ "$(RUST_VERSION)" = "$(MCP_VERSION)" ] && \
		[ "$(RUST_VERSION)" = "$(PYTHON_VERSION)" ] && \
		[ "$(RUST_VERSION)" = "$(NODE_VERSION)" ]; then \
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
	@if [ "$(RUST_VERSION)" != "$(PYTHON_VERSION)" ]; then \
		echo "ERROR: haiai ($(RUST_VERSION)) != python ($(PYTHON_VERSION))"; exit 1; fi
	@if [ "$(RUST_VERSION)" != "$(NODE_VERSION)" ]; then \
		echo "ERROR: haiai ($(RUST_VERSION)) != node ($(NODE_VERSION))"; exit 1; fi
	@echo "All versions match: $(RUST_VERSION)"

# ============================================================================
# GITHUB CI RELEASE (via git tags)
# ============================================================================
# Create git tags that trigger GitHub Actions release workflows.
# Versions are auto-detected from source files.
#
# Required GitHub Secrets:
#   - CRATES_IO_TOKEN  (for rust/v* tags)
#   - PYPI_API_TOKEN   (for python/v* tags, or use trusted publisher)
#   - NPM_TOKEN        (for node/v* tags)
# ============================================================================

check-version-rust:
	@echo "rust version: $(RUST_VERSION)"
	@if git tag -l | grep -q "^rust/v$(RUST_VERSION)$$"; then \
		echo "ERROR: Tag rust/v$(RUST_VERSION) already exists"; exit 1; fi
	@echo "Tag rust/v$(RUST_VERSION) is available"

check-version-python:
	@echo "python version: $(PYTHON_VERSION)"
	@if git tag -l | grep -q "^python/v$(PYTHON_VERSION)$$"; then \
		echo "ERROR: Tag python/v$(PYTHON_VERSION) already exists"; exit 1; fi
	@echo "Tag python/v$(PYTHON_VERSION) is available"

check-version-node:
	@echo "node version: $(NODE_VERSION)"
	@if git tag -l | grep -q "^node/v$(NODE_VERSION)$$"; then \
		echo "ERROR: Tag node/v$(NODE_VERSION) already exists"; exit 1; fi
	@echo "Tag node/v$(NODE_VERSION) is available"

release-rust: check-version-rust
	git tag rust/v$(RUST_VERSION)
	git push origin rust/v$(RUST_VERSION)
	@echo "Tagged rust/v$(RUST_VERSION) - CI will publish to crates.io + build CLI binaries"

release-python: check-version-python
	git tag python/v$(PYTHON_VERSION)
	git push origin python/v$(PYTHON_VERSION)
	@echo "Tagged python/v$(PYTHON_VERSION) - CI will publish to PyPI"

release-node: check-version-node
	git tag node/v$(NODE_VERSION)
	git push origin node/v$(NODE_VERSION)
	@echo "Tagged node/v$(NODE_VERSION) - CI will publish to npm"

release-all: check-versions release-rust release-python release-node
	@echo "All release tags pushed for v$(RUST_VERSION). CI will handle publishing."

# --- Retry targets (delete old tag, retag, push) ---

retry-rust:
	@echo "Retrying Rust release for v$(RUST_VERSION)..."
	-git tag -d rust/v$(RUST_VERSION)
	-git push origin --delete rust/v$(RUST_VERSION)
	git tag rust/v$(RUST_VERSION)
	git push origin rust/v$(RUST_VERSION)
	@echo "Re-tagged rust/v$(RUST_VERSION)"

retry-python:
	@echo "Retrying Python release for v$(PYTHON_VERSION)..."
	-git tag -d python/v$(PYTHON_VERSION)
	-git push origin --delete python/v$(PYTHON_VERSION)
	git tag python/v$(PYTHON_VERSION)
	git push origin python/v$(PYTHON_VERSION)
	@echo "Re-tagged python/v$(PYTHON_VERSION)"

retry-node:
	@echo "Retrying Node release for v$(NODE_VERSION)..."
	-git tag -d node/v$(NODE_VERSION)
	-git push origin --delete node/v$(NODE_VERSION)
	git tag node/v$(NODE_VERSION)
	git push origin node/v$(NODE_VERSION)
	@echo "Re-tagged node/v$(NODE_VERSION)"

release-delete-tags:
	@echo "Deleting tags for version $(RUST_VERSION)..."
	-git tag -d rust/v$(RUST_VERSION) python/v$(PYTHON_VERSION) node/v$(NODE_VERSION)
	-git push origin --delete rust/v$(RUST_VERSION) python/v$(PYTHON_VERSION) node/v$(NODE_VERSION)
	@echo "Deleted release tags"

# ============================================================================
# HELP
# ============================================================================

help:
	@echo "HAIAI SDK Makefile"
	@echo ""
	@echo "VERSION INFO:"
	@echo "  make versions        Show all detected versions"
	@echo "  make check-versions  Verify all package versions match"
	@echo ""
	@echo "TEST:"
	@echo "  make test            Run all tests"
	@echo "  make test-python     Run Python tests"
	@echo "  make test-node       Run Node tests"
	@echo "  make test-go         Run Go tests"
	@echo "  make test-rust       Run Rust tests"
	@echo ""
	@echo "RELEASE (via git tags, versions auto-detected):"
	@echo "  make release-rust    Tag rust/v<ver>   -> crates.io + CLI binaries"
	@echo "  make release-python  Tag python/v<ver> -> PyPI"
	@echo "  make release-node    Tag node/v<ver>   -> npm"
	@echo "  make release-all     Verify versions, then release all"
	@echo "  make release-delete-tags  Delete tags (fix failed releases)"
	@echo "  make retry-rust      Retry failed Rust release"
	@echo "  make retry-python    Retry failed Python release"
	@echo "  make retry-node      Retry failed Node release"
	@echo ""
	@echo "Required GitHub Secrets:"
	@echo "  CRATES_IO_TOKEN  - for rust/v* tags"
	@echo "  PYPI_API_TOKEN   - for python/v* tags (or trusted publisher)"
	@echo "  NPM_TOKEN        - for node/v* tags"
