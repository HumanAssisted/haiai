.PHONY: test test-python test-node test-go test-rust \
        versions check-versions \
        release-node release-python release-rust release-all \
        release-delete-tags help

# ============================================================================
# VERSION DETECTION
# ============================================================================

RUST_VERSION := $(shell grep '^version' rust/haiai/Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
CLI_VERSION := $(shell grep '^version' rust/haiai-cli/Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
MCP_VERSION := $(shell grep '^version' rust/hai-mcp/Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
PYTHON_VERSION := $(shell grep '^version' python/pyproject.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
NODE_VERSION := $(shell grep '"version"' node/package.json | head -1 | sed 's/.*: *"\(.*\)".*/\1/')
PLUGIN_VERSION := $(shell grep '"version"' .claude-plugin/plugin.json | head -1 | sed 's/.*: *"\(.*\)".*/\1/')

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
	@echo "  plugin          $(PLUGIN_VERSION)"
	@echo ""
	@if [ "$(RUST_VERSION)" = "$(CLI_VERSION)" ] && \
		[ "$(RUST_VERSION)" = "$(MCP_VERSION)" ] && \
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
	@if [ "$(RUST_VERSION)" != "$(PYTHON_VERSION)" ]; then \
		echo "ERROR: haiai ($(RUST_VERSION)) != python ($(PYTHON_VERSION))"; exit 1; fi
	@if [ "$(RUST_VERSION)" != "$(NODE_VERSION)" ]; then \
		echo "ERROR: haiai ($(RUST_VERSION)) != node ($(NODE_VERSION))"; exit 1; fi
	@if [ "$(RUST_VERSION)" != "$(PLUGIN_VERSION)" ]; then \
		echo "ERROR: haiai ($(RUST_VERSION)) != plugin ($(PLUGIN_VERSION))"; exit 1; fi
	@echo "All versions match: $(RUST_VERSION)"

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

release-all: check-versions release-rust
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
	@echo "RELEASE (via git tags, versions auto-detected, safe to re-run):"
	@echo "  make release-rust    Tag rust/v<ver>   -> crates.io + CLI binaries"
	@echo "  make release-python  Tag python/v<ver> -> PyPI"
	@echo "  make release-node    Tag node/v<ver>   -> npm"
	@echo "  make release-all     Verify versions, then release all"
	@echo "  make release-delete-tags  Delete all tags for current version"
	@echo ""
	@echo "Required GitHub Secrets:"
	@echo "  CRATES_IO_TOKEN  - for rust/v* tags"
	@echo "  PYPI_API_TOKEN   - for python/v* tags (or trusted publisher)"
	@echo "  NPM_TOKEN        - for node/v* tags"
