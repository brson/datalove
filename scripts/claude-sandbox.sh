#!/usr/bin/env bash
#
# Launch a sandboxed podman container for running Claude Code in yolo mode.
# - Only mounts the current directory
# - No SSH keyring access
# - Passes git and gh credentials
# - Supports container-in-container (podman in podman)
#
# Outer container requirements for nested podman:
# - Mount /dev/fuse device (--device /dev/fuse)
# - Disable SELinux labeling (--security-opt label=disable)
# - Use fuse-overlayfs or vfs storage driver

set -euo pipefail

REBUILD=false
while [[ $# -gt 0 ]]; do
    case "$1" in
        --rebuild) REBUILD=true; shift ;;
        *) echo "Unknown option: $1" >&2; exit 1 ;;
    esac
done

CONTAINER_NAME="claude-sandbox-$$"
WORKDIR="/workspace"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info() { echo -e "${GREEN}[*]${NC} $*"; }
warn() { echo -e "${YELLOW}[!]${NC} $*"; }
die() { echo -e "${RED}[ERROR]${NC} $*" >&2; exit 1; }

command -v podman >/dev/null || die "podman not found"

# Build volume mounts
mounts=(
    # Current directory only
    "-v" "$(pwd):${WORKDIR}:Z"
)

# Git config (no SSH keys)
[[ -f "$HOME/.gitconfig" ]] && mounts+=("-v" "$HOME/.gitconfig:/home/claude/.gitconfig:ro")

# gh CLI credentials
[[ -d "$HOME/.config/gh" ]] && mounts+=("-v" "$HOME/.config/gh:/home/claude/.config/gh:ro")

# Claude config/auth (read-write for OAuth tokens)
[[ -d "$HOME/.claude" ]] && mounts+=("-v" "$HOME/.claude:/home/claude/.claude")
[[ -f "$HOME/.claude.json" ]] && mounts+=("-v" "$HOME/.claude.json:/home/claude/.claude.json")

# Podman socket for container-in-container
podman_sock="/run/user/$(id -u)/podman/podman.sock"
[[ -S "$podman_sock" ]] && mounts+=("-v" "${podman_sock}:/run/podman/podman.sock")

# Environment variables
envs=(
    "-e" "TERM=${TERM:-xterm-256color}"
)
[[ -n "${ANTHROPIC_API_KEY:-}" ]] && envs+=("-e" "ANTHROPIC_API_KEY")
[[ -n "${GH_TOKEN:-}" ]] && envs+=("-e" "GH_TOKEN")
[[ -n "${GITHUB_TOKEN:-}" ]] && envs+=("-e" "GITHUB_TOKEN")

info "Sandbox: $(pwd) -> ${WORKDIR}"
info "Git config: $([[ -f "$HOME/.gitconfig" ]] && echo "yes" || echo "no")"
info "gh credentials: $([[ -d "$HOME/.config/gh" ]] && echo "yes" || echo "no")"
info "Claude config: $([[ -d "$HOME/.claude" ]] && echo "yes" || echo "no")"
info "Claude auth: $([[ -f "$HOME/.claude.json" ]] && echo "yes" || echo "no")"
info "SSH keys: no (intentionally excluded)"

# Dockerfile for the sandbox image
HOST_UID=$(id -u)
HOST_GID=$(id -g)

dockerfile=$(cat <<DOCKERFILE
FROM docker.io/library/ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive
ENV NVM_DIR=/home/claude/.nvm

RUN apt-get update && apt-get install -y --no-install-recommends \
    curl git ca-certificates fuse-overlayfs podman \
    && rm -rf /var/lib/apt/lists/*

# Install gh CLI
RUN curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg \
    | dd of=/usr/share/keyrings/githubcli-archive-keyring.gpg 2>/dev/null \
    && echo "deb [arch=\$(dpkg --print-architecture) signed-by=/usr/share/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" \
    > /etc/apt/sources.list.d/github-cli.list \
    && apt-get update && apt-get install -y --no-install-recommends gh \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user with matching UID/GID for --userns=keep-id
# Ubuntu 24.04 has ubuntu:1000 by default, so delete it first if it exists
RUN userdel -r ubuntu 2>/dev/null || true \
    && groupdel ubuntu 2>/dev/null || true \
    && groupadd -g ${HOST_GID} claude 2>/dev/null || true \
    && useradd -m -s /bin/bash -u ${HOST_UID} -g ${HOST_GID} claude

# Install nvm and node as claude user
USER claude
WORKDIR /home/claude
RUN curl -fsSL https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.1/install.sh | bash \
    && . "\$NVM_DIR/nvm.sh" \
    && nvm install 22 \
    && npm install -g @anthropic-ai/claude-code \
    && git config --global --add safe.directory /workspace \
    && echo 'export NVM_DIR="\$HOME/.nvm"' >> ~/.bashrc \
    && echo '[ -s "\$NVM_DIR/nvm.sh" ] && . "\$NVM_DIR/nvm.sh"' >> ~/.bashrc

WORKDIR /workspace
DOCKERFILE
)

# Build the image if needed (tagged with UID since it's baked in)
IMAGE_NAME="claude-sandbox:uid-${HOST_UID}"
if $REBUILD; then
    info "Rebuilding sandbox image..."
    echo "$dockerfile" | podman build --no-cache -t "$IMAGE_NAME" -f - .
elif ! podman image exists "$IMAGE_NAME" 2>/dev/null; then
    info "Building sandbox image (one-time)..."
    echo "$dockerfile" | podman build -t "$IMAGE_NAME" -f - .
fi

info "Starting sandbox..."

exec podman run -it --rm \
    --name "$CONTAINER_NAME" \
    --hostname "claude-sandbox" \
    --workdir "$WORKDIR" \
    --user claude \
    --userns=keep-id \
    --security-opt label=disable \
    --device /dev/fuse \
    "${mounts[@]}" \
    "${envs[@]}" \
    "$IMAGE_NAME" \
    bash -c '. ~/.nvm/nvm.sh && claude --dangerously-skip-permissions'