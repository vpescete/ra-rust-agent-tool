#!/bin/bash
set -e

echo "=== RA — Rust Agent Orchestrator ==="
echo ""

# Detect OS and architecture
OS="$(uname -s)"
ARCH="$(uname -m)"

case "${OS}" in
    Linux*)   PLATFORM="linux";;
    Darwin*)  PLATFORM="macos";;
    *)        echo "Unsupported OS: ${OS}"; exit 1;;
esac

case "${ARCH}" in
    x86_64)   ARCH_SUFFIX="x86_64";;
    aarch64)  ARCH_SUFFIX="arm64";;
    arm64)    ARCH_SUFFIX="arm64";;
    *)        echo "Unsupported architecture: ${ARCH}"; exit 1;;
esac

SUFFIX="${PLATFORM}-${ARCH_SUFFIX}"
INSTALL_DIR="${HOME}/.local/bin"

echo "Platform: ${PLATFORM}-${ARCH_SUFFIX}"
echo "Install directory: ${INSTALL_DIR}"
echo ""

# Check if claude is installed
if ! command -v claude &> /dev/null; then
    echo "WARNING: 'claude' CLI not found in PATH."
    echo "RA requires Claude Code CLI. Install it first:"
    echo "  npm install -g @anthropic-ai/claude-code"
    echo "  or: brew install claude-code"
    echo ""
fi

# Option 1: Install from GitHub releases
install_from_release() {
    REPO="vpescetelli/ra"
    LATEST=$(curl -s "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/')

    if [ -z "$LATEST" ]; then
        echo "Could not fetch latest release. Falling back to build from source."
        install_from_source
        return
    fi

    echo "Downloading RA ${LATEST}..."
    mkdir -p "${INSTALL_DIR}"

    curl -sL "https://github.com/${REPO}/releases/download/${LATEST}/ra-${SUFFIX}" -o "${INSTALL_DIR}/ra"
    curl -sL "https://github.com/${REPO}/releases/download/${LATEST}/ra-mcp-server-${SUFFIX}" -o "${INSTALL_DIR}/ra-mcp-server"
    chmod +x "${INSTALL_DIR}/ra" "${INSTALL_DIR}/ra-mcp-server"

    echo "Downloaded to ${INSTALL_DIR}"
}

# Option 2: Build from source
install_from_source() {
    if ! command -v cargo &> /dev/null; then
        echo "ERROR: Rust toolchain not found. Install it:"
        echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
        exit 1
    fi

    echo "Building from source..."
    cargo build --release -p ra-cli -p ra-mcp

    mkdir -p "${INSTALL_DIR}"
    cp target/release/ra "${INSTALL_DIR}/ra"
    cp target/release/ra-mcp-server "${INSTALL_DIR}/ra-mcp-server"
    chmod +x "${INSTALL_DIR}/ra" "${INSTALL_DIR}/ra-mcp-server"

    echo "Built and installed to ${INSTALL_DIR}"
}

# Determine install method
if [ "$1" = "--from-source" ]; then
    install_from_source
else
    install_from_release
fi

# Ensure install dir is in PATH
if ! echo "$PATH" | grep -q "${INSTALL_DIR}"; then
    echo ""
    echo "NOTE: ${INSTALL_DIR} is not in your PATH."
    echo "Add this to your shell profile (~/.zshrc or ~/.bashrc):"
    echo ""
    echo "  export PATH=\"\${HOME}/.local/bin:\${PATH}\""
    echo ""
fi

# Register MCP server in Claude Code
MCP_PATH="${INSTALL_DIR}/ra-mcp-server"
echo ""
echo "=== Register RA as Claude Code MCP server ==="
echo ""

if command -v claude &> /dev/null; then
    read -p "Register RA globally in Claude Code? [Y/n] " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]] || [[ -z $REPLY ]]; then
        claude mcp add ra "${MCP_PATH}" -s user 2>/dev/null || \
        claude mcp add ra "${MCP_PATH}" 2>/dev/null || \
        echo "Could not auto-register. Register manually:"
        echo "  claude mcp add ra ${MCP_PATH}"
    fi
else
    echo "Claude Code not found. After installing it, register RA with:"
    echo "  claude mcp add ra ${MCP_PATH}"
fi

echo ""
echo "=== Installation complete ==="
echo ""
echo "Usage:"
echo "  ra --help              # CLI help"
echo "  ra agent \"prompt\"      # Run single agent"
echo "  ra dashboard           # Live TUI dashboard"
echo ""
echo "In Claude Code, type /ra- to see available commands:"
echo "  /ra-review             # Pre-PR review"
echo "  /ra-bug-hunt           # Bug hunting"
echo "  /ra-templates          # List templates"
echo "  /ra-create-workflow    # Create custom workflow"
echo ""
