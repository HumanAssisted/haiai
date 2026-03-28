#!/bin/sh
# install.sh — Install or upgrade the haiai CLI from GitHub releases.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/HumanAssisted/haiai/main/install.sh | sh
#   curl -fsSL https://raw.githubusercontent.com/HumanAssisted/haiai/main/install.sh | sh -s -- --version 0.2.1
#   curl -fsSL https://raw.githubusercontent.com/HumanAssisted/haiai/main/install.sh | sh -s -- --dir ~/.local/bin
#
# Environment variables:
#   HAIAI_INSTALL_DIR   Override install directory (default: ~/.haiai/bin)
#   HAIAI_VERSION       Pin to a specific version (e.g., 0.2.1)

set -e

REPO="HumanAssisted/haiai"
BINARY_NAME="haiai"
DEFAULT_INSTALL_DIR="${HOME}/.haiai/bin"

# ── Helpers ──────────────────────────────────────────────────────

bold=""
reset=""
red=""
green=""
yellow=""
if [ -t 1 ]; then
    bold="\033[1m"
    reset="\033[0m"
    red="\033[31m"
    green="\033[32m"
    yellow="\033[33m"
fi

info()    { printf "${bold}${green}info${reset}  %s\n" "$1"; }
warn()    { printf "${bold}${yellow}warn${reset}  %s\n" "$1"; }
error()   { printf "${bold}${red}error${reset} %s\n" "$1" >&2; exit 1; }

need_cmd() {
    if ! command -v "$1" > /dev/null 2>&1; then
        error "Required command not found: $1"
    fi
}

# ── Parse arguments ──────────────────────────────────────────────

VERSION=""
INSTALL_DIR=""
while [ $# -gt 0 ]; do
    case "$1" in
        --version)  VERSION="$2"; shift 2 ;;
        --dir)      INSTALL_DIR="$2"; shift 2 ;;
        -h|--help)
            echo "Usage: install.sh [--version VERSION] [--dir DIRECTORY]"
            echo ""
            echo "Options:"
            echo "  --version VERSION   Install a specific version (default: latest)"
            echo "  --dir DIRECTORY     Install to DIRECTORY (default: ~/.haiai/bin)"
            echo ""
            echo "Environment variables:"
            echo "  HAIAI_INSTALL_DIR   Same as --dir"
            echo "  HAIAI_VERSION       Same as --version"
            exit 0
            ;;
        *)          error "Unknown argument: $1" ;;
    esac
done

VERSION="${VERSION:-${HAIAI_VERSION:-}}"
INSTALL_DIR="${INSTALL_DIR:-${HAIAI_INSTALL_DIR:-${DEFAULT_INSTALL_DIR}}}"

# ── Check dependencies ───────────────────────────────────────────

need_cmd curl
need_cmd tar
need_cmd uname

# ── Detect platform ──────────────────────────────────────────────

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
    Darwin)
        case "$ARCH" in
            arm64)   ASSET_SUFFIX="darwin-arm64" ;;
            x86_64)  ASSET_SUFFIX="darwin-x64" ;;
            *)       error "Unsupported macOS architecture: $ARCH" ;;
        esac
        ;;
    Linux)
        case "$ARCH" in
            x86_64)  ASSET_SUFFIX="linux-x64" ;;
            aarch64) ASSET_SUFFIX="linux-arm64" ;;
            *)       error "Unsupported Linux architecture: $ARCH" ;;
        esac
        ;;
    MINGW*|MSYS*|CYGWIN*)
        error "Windows is not supported by this script. Download from: https://github.com/${REPO}/releases"
        ;;
    *)
        error "Unsupported operating system: $OS"
        ;;
esac

info "Detected platform: ${OS} ${ARCH} (${ASSET_SUFFIX})"

# ── Resolve version ──────────────────────────────────────────────

if [ -z "$VERSION" ]; then
    info "Fetching latest version..."
    VERSION="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases?per_page=20" \
        | grep '"tag_name":' \
        | grep 'rust/v' \
        | head -1 \
        | sed 's/.*"rust\/v\([^"]*\)".*/\1/')"
    if [ -z "$VERSION" ]; then
        error "Could not determine latest version. Specify one with --version."
    fi
fi

# Strip leading 'v' if someone passes v0.2.1
VERSION="$(echo "$VERSION" | sed 's/^v//')"

info "Version: ${VERSION}"

# ── Check existing installation ──────────────────────────────────

EXISTING=""
# Check the install directory first (may not be on PATH yet), then PATH
if [ -x "${INSTALL_DIR}/${BINARY_NAME}" ]; then
    EXISTING="$("${INSTALL_DIR}/${BINARY_NAME}" --version 2>/dev/null | head -1 | sed 's/[^0-9.]//g' || true)"
elif command -v "$BINARY_NAME" > /dev/null 2>&1; then
    EXISTING="$(${BINARY_NAME} --version 2>/dev/null | head -1 | sed 's/[^0-9.]//g' || true)"
fi

if [ -n "$EXISTING" ]; then
    if [ "$EXISTING" = "$VERSION" ]; then
        info "haiai ${VERSION} is already installed"
        exit 0
    fi
    # Compare versions to distinguish upgrade from downgrade
    EXISTING_MAJOR="$(echo "$EXISTING" | cut -d. -f1)"
    EXISTING_MINOR="$(echo "$EXISTING" | cut -d. -f2)"
    EXISTING_PATCH="$(echo "$EXISTING" | cut -d. -f3)"
    TARGET_MAJOR="$(echo "$VERSION" | cut -d. -f1)"
    TARGET_MINOR="$(echo "$VERSION" | cut -d. -f2)"
    TARGET_PATCH="$(echo "$VERSION" | cut -d. -f3)"

    IS_DOWNGRADE=0
    if [ "$TARGET_MAJOR" -lt "$EXISTING_MAJOR" ] 2>/dev/null; then
        IS_DOWNGRADE=1
    elif [ "$TARGET_MAJOR" -eq "$EXISTING_MAJOR" ] && [ "$TARGET_MINOR" -lt "$EXISTING_MINOR" ] 2>/dev/null; then
        IS_DOWNGRADE=1
    elif [ "$TARGET_MAJOR" -eq "$EXISTING_MAJOR" ] && [ "$TARGET_MINOR" -eq "$EXISTING_MINOR" ] && [ "$TARGET_PATCH" -lt "$EXISTING_PATCH" ] 2>/dev/null; then
        IS_DOWNGRADE=1
    fi

    if [ "$IS_DOWNGRADE" -eq 1 ]; then
        warn "Downgrading haiai ${EXISTING} -> ${VERSION}"
    else
        info "Upgrading haiai ${EXISTING} -> ${VERSION}"
    fi
else
    info "Installing haiai ${VERSION}"
fi

# ── Download ─────────────────────────────────────────────────────

ASSET_NAME="haiai-cli-${VERSION}-${ASSET_SUFFIX}.tar.gz"
RELEASE_BASE="https://github.com/${REPO}/releases/download/rust/v${VERSION}"
DOWNLOAD_URL="${RELEASE_BASE}/${ASSET_NAME}"
CHECKSUMS_URL="${RELEASE_BASE}/sha256sums.txt"

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

info "Downloading ${ASSET_NAME}..."
curl -fsSL --proto '=https' --tlsv1.2 --retry 3 \
    -o "${TMPDIR}/${ASSET_NAME}" "$DOWNLOAD_URL" \
    || error "Download failed. Check that version ${VERSION} exists at:\n  ${DOWNLOAD_URL}"

# ── Verify checksum ──────────────────────────────────────────────

info "Verifying checksum..."
curl -fsSL --proto '=https' --tlsv1.2 --retry 3 \
    -o "${TMPDIR}/sha256sums.txt" "$CHECKSUMS_URL" 2>/dev/null || true

if [ -f "${TMPDIR}/sha256sums.txt" ]; then
    EXPECTED="$(grep "${ASSET_NAME}" "${TMPDIR}/sha256sums.txt" | awk '{print $1}')"
    if [ -z "$EXPECTED" ]; then
        warn "Asset not found in sha256sums.txt, skipping verification"
    else
        if command -v sha256sum > /dev/null 2>&1; then
            ACTUAL="$(sha256sum "${TMPDIR}/${ASSET_NAME}" | awk '{print $1}')"
        elif command -v shasum > /dev/null 2>&1; then
            ACTUAL="$(shasum -a 256 "${TMPDIR}/${ASSET_NAME}" | awk '{print $1}')"
        else
            warn "No sha256sum or shasum found, skipping verification"
            ACTUAL="$EXPECTED"
        fi
        if [ "$EXPECTED" != "$ACTUAL" ]; then
            error "Checksum mismatch!\n  Expected: ${EXPECTED}\n  Actual:   ${ACTUAL}"
        fi
        info "Checksum verified"
    fi
else
    warn "Checksums not available, skipping verification"
fi

# ── Extract and install ──────────────────────────────────────────

tar xzf "${TMPDIR}/${ASSET_NAME}" -C "$TMPDIR"

# The archive contains haiai-cli, rename to haiai
if [ -f "${TMPDIR}/haiai-cli" ]; then
    mv "${TMPDIR}/haiai-cli" "${TMPDIR}/${BINARY_NAME}"
elif [ ! -f "${TMPDIR}/${BINARY_NAME}" ]; then
    error "Binary not found in archive"
fi

mkdir -p "$INSTALL_DIR"
mv "${TMPDIR}/${BINARY_NAME}" "${INSTALL_DIR}/${BINARY_NAME}"
chmod +x "${INSTALL_DIR}/${BINARY_NAME}"

info "Installed to ${INSTALL_DIR}/${BINARY_NAME}"

# ── Update PATH ──────────────────────────────────────────────────

case ":${PATH}:" in
    *":${INSTALL_DIR}:"*) ;;
    *)
        SHELL_NAME="$(basename "${SHELL:-/bin/sh}")"
        PROFILE=""
        case "$SHELL_NAME" in
            zsh)  PROFILE="${HOME}/.zshrc" ;;
            bash)
                if [ -f "${HOME}/.bash_profile" ]; then
                    PROFILE="${HOME}/.bash_profile"
                else
                    PROFILE="${HOME}/.bashrc"
                fi
                ;;
            fish) PROFILE="${HOME}/.config/fish/config.fish" ;;
        esac

        EXPORT_LINE="export PATH=\"${INSTALL_DIR}:\$PATH\""
        if [ "$SHELL_NAME" = "fish" ]; then
            EXPORT_LINE="set -gx PATH ${INSTALL_DIR} \$PATH"
        fi

        if [ -n "$PROFILE" ] && [ -w "$PROFILE" ]; then
            if ! grep -qF "$INSTALL_DIR" "$PROFILE" 2>/dev/null; then
                printf "\n# haiai CLI\n%s\n" "$EXPORT_LINE" >> "$PROFILE"
                info "Added ${INSTALL_DIR} to PATH in ${PROFILE}"
            fi
        else
            warn "Add this to your shell profile:"
            printf "  %s\n" "$EXPORT_LINE"
        fi

        warn "Restart your shell or run: ${EXPORT_LINE}"
        ;;
esac

# ── Verify ───────────────────────────────────────────────────────

if "${INSTALL_DIR}/${BINARY_NAME}" --version > /dev/null 2>&1; then
    INSTALLED_VERSION="$("${INSTALL_DIR}/${BINARY_NAME}" --version 2>&1 | head -1)"
    info "Success! ${INSTALLED_VERSION}"
else
    warn "Binary installed but --version check failed. You may need to install system libraries."
fi
