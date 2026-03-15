#!/usr/bin/env bash
# ╔══════════════════════════════════════════════════════════════════╗
# ║           RustFlow Installer — install.sh                        ║
# ║                                                                  ║
# ║  Usage (from repo root):                                         ║
# ║    ./install.sh                                                  ║
# ║    ./install.sh --prefix /usr/local                              ║
# ║    ./install.sh --no-modify-path                                 ║
# ║                                                                  ║
# ║  Usage (one-liner):                                              ║
# ║    curl -fsSL https://raw.githubusercontent.com/rust-ai-flow/   ║
# ║         rustflow/master/install.sh | bash                        ║
# ╚══════════════════════════════════════════════════════════════════╝
set -euo pipefail

# ── Constants ─────────────────────────────────────────────────────────────────

REPO_URL="https://github.com/rust-ai-flow/rustflow.git"
BINARY_NAME="rustflow"
DEFAULT_PREFIX="$HOME/.rustflow"

# ── Colors ────────────────────────────────────────────────────────────────────

if [ -t 1 ]; then
  BOLD="\033[1m"
  DIM="\033[2m"
  RED="\033[31m"
  GREEN="\033[32m"
  YELLOW="\033[33m"
  CYAN="\033[36m"
  RESET="\033[0m"
else
  BOLD="" DIM="" RED="" GREEN="" YELLOW="" CYAN="" RESET=""
fi

# ── Logging ───────────────────────────────────────────────────────────────────

info()    { printf "${CYAN}  •${RESET}  %s\n" "$*"; }
success() { printf "${GREEN}  ✓${RESET}  %s\n" "$*"; }
warn()    { printf "${YELLOW}  !${RESET}  %s\n" "$*"; }
error()   { printf "${RED}  ✗${RESET}  %s\n" "$*" >&2; }
step()    { printf "\n${BOLD}%s${RESET}\n" "$*"; }
die()     { error "$*"; exit 1; }

# ── Argument parsing ──────────────────────────────────────────────────────────

PREFIX="$DEFAULT_PREFIX"
MODIFY_PATH=true
VERBOSE=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --prefix)       PREFIX="$2"; shift 2 ;;
    --prefix=*)     PREFIX="${1#*=}"; shift ;;
    --no-modify-path) MODIFY_PATH=false; shift ;;
    --verbose | -v)   VERBOSE=true; shift ;;
    --help | -h)
      echo "Usage: $0 [OPTIONS]"
      echo ""
      echo "Options:"
      echo "  --prefix <dir>      Install directory (default: \$HOME/.rustflow)"
      echo "  --no-modify-path    Do not modify shell profile"
      echo "  --verbose, -v       Show cargo build output"
      echo "  --help, -h          Show this help"
      exit 0
      ;;
    *) die "Unknown option: $1" ;;
  esac
done

BIN_DIR="$PREFIX/bin"

# ── Header ────────────────────────────────────────────────────────────────────

printf "\n"
printf "${BOLD}${CYAN}"
printf "  ██████╗ ██╗   ██╗███████╗████████╗███████╗██╗      ██████╗ ██╗    ██╗\n"
printf "  ██╔══██╗██║   ██║██╔════╝╚══██╔══╝██╔════╝██║     ██╔═══██╗██║    ██║\n"
printf "  ██████╔╝██║   ██║███████╗   ██║   █████╗  ██║     ██║   ██║██║ █╗ ██║\n"
printf "  ██╔══██╗██║   ██║╚════██║   ██║   ██╔══╝  ██║     ██║   ██║██║███╗██║\n"
printf "  ██║  ██║╚██████╔╝███████║   ██║   ██║     ███████╗╚██████╔╝╚███╔███╔╝\n"
printf "  ╚═╝  ╚═╝ ╚═════╝ ╚══════╝   ╚═╝   ╚═╝     ╚══════╝ ╚═════╝  ╚══╝╚══╝ \n"
printf "${RESET}"
printf "\n  ${DIM}High-performance AI Agent orchestration runtime${RESET}\n\n"

# ── OS / arch detection ───────────────────────────────────────────────────────

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Linux)  OS_LABEL="Linux" ;;
  Darwin) OS_LABEL="macOS" ;;
  *)      die "Unsupported OS: $OS. Only Linux and macOS are supported." ;;
esac

case "$ARCH" in
  x86_64 | amd64)  ARCH_LABEL="x86_64" ;;
  arm64  | aarch64) ARCH_LABEL="arm64" ;;
  *)                die "Unsupported architecture: $ARCH." ;;
esac

info "Platform: ${OS_LABEL} ${ARCH_LABEL}"
info "Install prefix: ${BIN_DIR}"

# ── Dependency checks ─────────────────────────────────────────────────────────

step "Checking dependencies"

# cargo
if ! command -v cargo &>/dev/null; then
  warn "Cargo not found."
  printf "\n  Install Rust via rustup? (y/N) "
  read -r answer </dev/tty
  if [[ "$answer" =~ ^[Yy]$ ]]; then
    info "Installing rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path
    # shellcheck source=/dev/null
    source "$HOME/.cargo/env"
  else
    die "Cargo is required. Install it from https://rustup.rs and try again."
  fi
fi

CARGO_VERSION="$(cargo --version)"
success "Cargo: ${CARGO_VERSION}"

# git (only needed if we have to clone)
if ! command -v git &>/dev/null; then
  die "Git is required but not installed."
fi

# ── Source location ───────────────────────────────────────────────────────────

step "Locating source"

# Detect whether we're already running from inside the repo.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" && pwd)"
if [[ -f "$SCRIPT_DIR/Cargo.toml" ]] && grep -q 'rustflow-cli' "$SCRIPT_DIR/Cargo.toml" 2>/dev/null; then
  SOURCE_DIR="$SCRIPT_DIR"
  CLONED=false
  success "Using repo at: ${SOURCE_DIR}"
else
  # Running via curl pipe — clone to a temp dir.
  TMP_DIR="$(mktemp -d)"
  trap 'rm -rf "$TMP_DIR"' EXIT
  SOURCE_DIR="$TMP_DIR/rustflow"
  CLONED=true
  info "Cloning ${REPO_URL} ..."
  git clone --depth 1 "$REPO_URL" "$SOURCE_DIR" \
    ${VERBOSE:+--verbose} 2>&1 | (${VERBOSE} && cat || grep -E '^(Cloning|done\.|error)' || true)
  success "Cloned to: ${SOURCE_DIR}"
fi

# ── Build ─────────────────────────────────────────────────────────────────────

step "Building rustflow (release)"

info "Running: cargo build --release -p rustflow-cli"
info "This may take a few minutes on first build..."

BUILD_CMD=(cargo build --release -p rustflow-cli)

if $VERBOSE; then
  (cd "$SOURCE_DIR" && "${BUILD_CMD[@]}")
else
  (cd "$SOURCE_DIR" && "${BUILD_CMD[@]}" 2>&1) | \
    grep -E '^\s*(Compiling|Finished|error|warning.*unused)' || true
fi

BUILT_BIN="$SOURCE_DIR/target/release/$BINARY_NAME"
[[ -f "$BUILT_BIN" ]] || die "Build succeeded but binary not found at: $BUILT_BIN"

BINARY_SIZE="$(du -sh "$BUILT_BIN" | cut -f1)"
success "Built: ${BUILT_BIN} (${BINARY_SIZE})"

# ── Install ───────────────────────────────────────────────────────────────────

step "Installing binary"

mkdir -p "$BIN_DIR"
INSTALL_PATH="$BIN_DIR/$BINARY_NAME"

cp "$BUILT_BIN" "$INSTALL_PATH"
chmod +x "$INSTALL_PATH"
success "Installed: ${INSTALL_PATH}"

# ── PATH setup ────────────────────────────────────────────────────────────────

# Check whether BIN_DIR is already in PATH.
path_contains() { [[ ":${PATH}:" == *":$1:"* ]]; }

if path_contains "$BIN_DIR"; then
  success "PATH already contains: ${BIN_DIR}"
  MODIFY_PATH=false
fi

if $MODIFY_PATH; then
  step "Updating shell profile"

  # Determine which shell profiles to update.
  SHELL_PROFILES=()
  CURRENT_SHELL="$(basename "${SHELL:-/bin/sh}")"

  case "$CURRENT_SHELL" in
    zsh)
      [[ -f "$HOME/.zshrc" ]]     && SHELL_PROFILES+=("$HOME/.zshrc")
      [[ -f "$HOME/.zprofile" ]]  && SHELL_PROFILES+=("$HOME/.zprofile")
      ;;
    bash)
      [[ -f "$HOME/.bashrc" ]]    && SHELL_PROFILES+=("$HOME/.bashrc")
      [[ -f "$HOME/.bash_profile" ]] && SHELL_PROFILES+=("$HOME/.bash_profile")
      ;;
  esac

  # Fallback: always touch ~/.profile
  [[ ${#SHELL_PROFILES[@]} -eq 0 ]] && SHELL_PROFILES+=("$HOME/.profile")

  EXPORT_LINE="export PATH=\"\$PATH:${BIN_DIR}\""
  MARKER="# Added by RustFlow installer"

  for profile in "${SHELL_PROFILES[@]}"; do
    if grep -qF "$BIN_DIR" "$profile" 2>/dev/null; then
      info "Already present in: ${profile}"
    else
      printf "\n%s\n%s\n" "$MARKER" "$EXPORT_LINE" >> "$profile"
      success "Updated: ${profile}"
    fi
  done

  warn "Restart your shell or run: ${BOLD}source ~/.${CURRENT_SHELL}rc${RESET}"
fi

# ── Verify ────────────────────────────────────────────────────────────────────

step "Verifying installation"

# Temporarily add to PATH so we can call it right now.
export PATH="$BIN_DIR:$PATH"

if command -v "$BINARY_NAME" &>/dev/null; then
  VERSION="$("$BINARY_NAME" --version 2>/dev/null || echo "unknown")"
  success "rustflow ${VERSION} is ready"
else
  warn "Binary installed but not yet in PATH — restart your shell."
fi

# ── Done ──────────────────────────────────────────────────────────────────────

printf "\n"
printf "  ${BOLD}${GREEN}Installation complete!${RESET}\n\n"
printf "  ${DIM}Quick start:${RESET}\n\n"
printf "    ${CYAN}rustflow doctor${RESET}              Check your environment\n"
printf "    ${CYAN}rustflow init my-agent${RESET}        Create a new project\n"
printf "    ${CYAN}rustflow run workflow.yaml${RESET}    Execute a workflow\n"
printf "    ${CYAN}rustflow serve${RESET}                Start the HTTP API server\n"
printf "\n"
