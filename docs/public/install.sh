#!/bin/bash
set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Configuration
REPO="rizqme/raya"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.raya/bin}"
VERSION="${VERSION:-latest}"
DRY_RUN=false

# Print functions
info() { echo -e "${GREEN}[INFO]${NC} $1"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
error() { echo -e "${RED}[ERROR]${NC} $1"; exit 1; }

# Help message
show_help() {
  cat << EOF
Install Raya programming language

USAGE:
    curl -fsSL https://raya.land/install.sh | sh [OPTIONS]

OPTIONS:
    --version <version>    Install specific version (default: latest)
    --dir <directory>      Installation directory (default: ~/.raya/bin)
    --dry-run              Show what would be installed without installing
    --help                 Show this help message

ENVIRONMENT VARIABLES:
    VERSION                Version to install
    INSTALL_DIR            Installation directory

EXAMPLES:
    curl -fsSL https://raya.land/install.sh | sh
    curl -fsSL https://raya.land/install.sh | sh -s -- --version v0.1.0
    INSTALL_DIR=/usr/local/bin curl -fsSL https://raya.land/install.sh | sh
EOF
}

# Parse arguments
while [[ $# -gt 0 ]]; do
  case $1 in
    --version)
      VERSION="$2"
      shift 2
      ;;
    --dir)
      INSTALL_DIR="$2"
      shift 2
      ;;
    --dry-run)
      DRY_RUN=true
      shift
      ;;
    --help)
      show_help
      exit 0
      ;;
    *)
      error "Unknown option: $1. Use --help for usage information."
      ;;
  esac
done

# Detect OS
detect_os() {
  case "$(uname -s)" in
    Linux*)  echo "linux" ;;
    Darwin*) echo "macos" ;;
    *)       error "Unsupported OS: $(uname -s)" ;;
  esac
}

# Detect architecture
detect_arch() {
  case "$(uname -m)" in
    x86_64|amd64) echo "x86_64" ;;
    aarch64|arm64) echo "aarch64" ;;
    *) error "Unsupported architecture: $(uname -m)" ;;
  esac
}

# Get latest version from GitHub
get_latest_version() {
  info "Fetching latest version from GitHub..."
  if command -v curl > /dev/null; then
    curl -s "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/'
  elif command -v wget > /dev/null; then
    wget -qO- "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/'
  else
    error "Neither curl nor wget is available"
  fi
}

# Main installation
main() {
  OS=$(detect_os)
  ARCH=$(detect_arch)

  if [ "$VERSION" = "latest" ]; then
    VERSION=$(get_latest_version)
    if [ -z "$VERSION" ]; then
      error "Failed to fetch latest version"
    fi
  fi

  # Construct asset name
  if [ "$OS" = "linux" ]; then
    PLATFORM="ubuntu"
  else
    PLATFORM="macos"
  fi

  ASSET_NAME="raya-${PLATFORM}-${ARCH}.tar.gz"
  DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION}/${ASSET_NAME}"
  CHECKSUMS_URL="https://github.com/${REPO}/releases/download/${VERSION}/checksums.txt"

  info "Raya Installer"
  info "=============="
  info "Version:    ${VERSION}"
  info "Platform:   ${OS} ${ARCH}"
  info "Install to: ${INSTALL_DIR}"
  echo

  if [ "$DRY_RUN" = true ]; then
    info "Dry run mode - would download:"
    info "  ${DOWNLOAD_URL}"
    exit 0
  fi

  # Create temp directory
  TMP_DIR=$(mktemp -d)
  trap "rm -rf $TMP_DIR" EXIT

  info "Downloading ${ASSET_NAME}..."
  if command -v curl > /dev/null; then
    curl -L -o "${TMP_DIR}/${ASSET_NAME}" "${DOWNLOAD_URL}"
    curl -L -o "${TMP_DIR}/checksums.txt" "${CHECKSUMS_URL}"
  elif command -v wget > /dev/null; then
    wget -O "${TMP_DIR}/${ASSET_NAME}" "${DOWNLOAD_URL}"
    wget -O "${TMP_DIR}/checksums.txt" "${CHECKSUMS_URL}"
  else
    error "Neither curl nor wget is available"
  fi

  # Verify checksum
  info "Verifying checksum..."
  cd "${TMP_DIR}"
  if ! grep "${ASSET_NAME}" checksums.txt > expected_checksum.txt 2>/dev/null; then
    warn "Checksum for ${ASSET_NAME} not found in checksums.txt"
    warn "Skipping checksum verification"
  else
    if command -v sha256sum > /dev/null; then
      sha256sum -c expected_checksum.txt || error "Checksum verification failed"
    elif command -v shasum > /dev/null; then
      shasum -a 256 -c expected_checksum.txt || error "Checksum verification failed"
    else
      warn "No checksum tool available, skipping verification"
    fi
  fi

  # Extract
  info "Extracting..."
  tar xzf "${ASSET_NAME}"

  # Create install directory
  info "Installing to ${INSTALL_DIR}..."
  mkdir -p "${INSTALL_DIR}"
  mv raya "${INSTALL_DIR}/"

  # Make executable
  chmod +x "${INSTALL_DIR}/raya"

  # Update PATH
  update_path

  info "Installation complete!"
  info
  info "Run 'raya --version' to verify."
  info "Run 'raya --help' for usage information."
}

# Update PATH in shell config
update_path() {
  # Check if INSTALL_DIR is already in PATH
  if echo ":$PATH:" | grep -q ":${INSTALL_DIR}:"; then
    info "${INSTALL_DIR} is already in PATH"
    return
  fi

  # Add to PATH
  for SHELL_RC in "$HOME/.bashrc" "$HOME/.zshrc" "$HOME/.profile"; do
    if [ -f "$SHELL_RC" ]; then
      if ! grep -q "${INSTALL_DIR}" "$SHELL_RC"; then
        echo "" >> "$SHELL_RC"
        echo "# Raya" >> "$SHELL_RC"
        echo "export PATH=\"${INSTALL_DIR}:\$PATH\"" >> "$SHELL_RC"
        info "Added to PATH in ${SHELL_RC}"
        info "Run 'source ${SHELL_RC}' or restart your shell"
        return
      fi
    fi
  done

  warn "Could not find shell config file. Add ${INSTALL_DIR} to PATH manually:"
  warn "  export PATH=\"${INSTALL_DIR}:\$PATH\""
}

main
