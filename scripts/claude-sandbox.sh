#!/usr/bin/env bash
#
# Launch a sandboxed podman container for Claude Code with --dangerously-skip-permissions.
#
# Usage: claude-sandbox.sh [--rebuild] [claude|bash]
#
# Container image (Ubuntu 24.04):
#   - Build tools: gcc, clang, cmake, pkg-config, libssl-dev
#   - JDK + Maven (default-jdk)
#   - Editors: nano, emacs-nox
#   - GitHub CLI (gh)
#   - Claude Code (native binary via install.sh)
#   - Notification chime (pw-play / paplay / terminal bell fallback)
#
# Security:
#   - Runs as non-root user "claude" with host UID/GID (--userns=keep-id)
#   - Mounts only the current directory (at host path, for unique project identity)
#   - No SSH keys (intentionally excluded)
#   - Custom seccomp profile (claude-sandbox-seccomp.json), allows io_uring
#   - SELinux labels disabled (--security-opt label=disable)
#
# Resource limits:
#   - 4 CPUs, cpu-shares=512, 8 GB memory
#
# Passthrough (from host, when available):
#   - Git config (~/.gitconfig, ~/.gitignore) - read-only
#   - Claude config/auth (~/.claude, ~/.claude.json) - read-write
#   - Claude settings overridden with claude-sandbox-settings.json (hooks for chime)
#   - Rust toolchain (~/.rustup, ~/.cargo) - cargo config.toml masked with /dev/null
#   - GPU access (/dev/dri, video group, Vulkan/Mesa drivers)
#   - Wayland display socket
#   - PipeWire audio socket

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

REBUILD=false
COMMAND="claude"
while [[ $# -gt 0 ]]; do
    case "$1" in
        --rebuild) REBUILD=true; shift ;;
        claude|bash) COMMAND="$1"; shift ;;
        *) echo "Usage: $0 [--rebuild] [claude|bash]" >&2; exit 1 ;;
    esac
done

CONTAINER_NAME="claude-sandbox-$$"
# Use the host path as the container mount point so Claude Code derives a
# unique project identity per directory (instead of everything being "/workspace").
WORKDIR="$(pwd)"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info() { echo -e "${GREEN}[*]${NC} $*"; }
warn() { echo -e "${YELLOW}[!]${NC} $*"; }
die() { echo -e "${RED}[ERROR]${NC} $*" >&2; exit 1; }

command -v podman >/dev/null || die "podman not found"

# Mounting $HOME as the workspace conflicts with individual home directory mounts
# (.claude, .cargo, .rustup, etc.) and causes podman to hang.
[[ "$(pwd)" == "$HOME" ]] && die "refusing to run from home directory -- cd into a project first"

# Volume mounts
mounts=(
    "-v" "$(pwd):${WORKDIR}:Z"
)

# Git config (no SSH keys)
[[ -f "$HOME/.gitconfig" ]] && mounts+=("-v" "$HOME/.gitconfig:/home/claude/.gitconfig:ro")
[[ -f "$HOME/.gitignore" ]] && mounts+=("-v" "$HOME/.gitignore:/home/claude/.gitignore:ro")

# Claude config/auth (read-write for OAuth tokens)
[[ -d "$HOME/.claude" ]] && mounts+=("-v" "$HOME/.claude:/home/claude/.claude")
[[ -f "$HOME/.claude.json" ]] && mounts+=("-v" "$HOME/.claude.json:/home/claude/.claude.json")
# Claude binary (read-only to prevent in-sandbox upgrades from breaking host symlink)
[[ -d "$HOME/.local/bin" ]] && mounts+=("-v" "$HOME/.local/bin:/home/claude/.local/bin:ro")
[[ -d "$HOME/.local/share/claude" ]] && mounts+=("-v" "$HOME/.local/share/claude:/home/claude/.local/share/claude:ro")
[[ -d "$HOME/.local/share/claude" ]] && mounts+=("-v" "$HOME/.local/share/claude:$HOME/.local/share/claude:ro")
# Override settings.json with container-specific paths for hooks
[[ -f "$SCRIPT_DIR/claude-sandbox-settings.json" ]] && mounts+=("-v" "$SCRIPT_DIR/claude-sandbox-settings.json:/home/claude/.claude/settings.json:ro")

# PipeWire audio socket for notification chimes
XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
[[ -S "$XDG_RUNTIME_DIR/pipewire-0" ]] && mounts+=("-v" "$XDG_RUNTIME_DIR/pipewire-0:/run/user/1000/pipewire-0")

# Wayland display socket for GUI apps
WAYLAND_DISPLAY="${WAYLAND_DISPLAY:-wayland-0}"
[[ -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" ]] && mounts+=("-v" "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY:/run/user/1000/$WAYLAND_DISPLAY")

# Rust toolchain (mask config.toml to avoid host-specific paths)
[[ -d "$HOME/.rustup" ]] && mounts+=("-v" "$HOME/.rustup:/home/claude/.rustup")
[[ -d "$HOME/.cargo" ]] && mounts+=("-v" "$HOME/.cargo:/home/claude/.cargo")
[[ -d "$HOME/.cargo" ]] && mounts+=("-v" "/dev/null:/home/claude/.cargo/config.toml:ro")

# Environment variables
envs=(
    "-e" "TERM=${TERM:-xterm-256color}"
    "-e" "RUSTUP_HOME=/home/claude/.rustup"
    "-e" "CARGO_HOME=/home/claude/.cargo"
    "-e" "JAVA_HOME=/usr/lib/jvm/default-java"
    "-e" "XDG_RUNTIME_DIR=/run/user/1000"
    "-e" "WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-wayland-0}"
)
info "Sandbox: $(pwd) -> ${WORKDIR}"
info "Git config: $([[ -f "$HOME/.gitconfig" ]] && echo "yes" || echo "no")"
info "Claude config: $([[ -d "$HOME/.claude" ]] && echo "yes" || echo "no")"
info "Rust toolchain: $([[ -d "$HOME/.rustup" ]] && echo "yes" || echo "no")"
info "GPU: $([[ -e /dev/dri ]] && echo "yes" || echo "no")"
info "Wayland: $([[ -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" ]] && echo "yes" || echo "no")"
info "PipeWire: $([[ -S "$XDG_RUNTIME_DIR/pipewire-0" ]] && echo "yes" || echo "no")"

# Dockerfile for the sandbox image
HOST_UID=$(id -u)
HOST_GID=$(id -g)

dockerfile=$(cat <<DOCKERFILE
FROM docker.io/library/ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
    curl git ca-certificates build-essential clang cmake pkg-config libssl-dev nano emacs-nox \
    default-jdk maven \
    pipewire pipewire-audio-client-libraries \
    libwayland-client0 libwayland-cursor0 libwayland-egl1 libxkbcommon0 \
    mesa-vulkan-drivers libvulkan1 libasound2-dev \
    xvfb imagemagick mesa-utils libgl1-mesa-dri libegl1-mesa \
    && rm -rf /var/lib/apt/lists/*

ENV JAVA_HOME=/usr/lib/jvm/default-java

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

# Install notification chime assets
COPY --chown=claude:claude claude-chime-notify.sh /home/claude/.local/bin/claude-chime-notify
COPY --chown=claude:claude chime.wav /home/claude/.local/share/sounds/chime.wav

# Install Claude Code native binary
USER claude
WORKDIR /home/claude
RUN curl -fsSL https://claude.ai/install.sh | bash \
    && git config --global --add safe.directory '*'

WORKDIR /home/claude
DOCKERFILE
)

# Build the image if needed (tagged with UID since it's baked in)
IMAGE_NAME="claude-sandbox:uid-${HOST_UID}"
# Use minimal build context with just the assets needed
build_image() {
    local ctx
    ctx=$(mktemp -d)
    cp "$SCRIPT_DIR/claude-chime-notify.sh" "$ctx/"
    cp "$SCRIPT_DIR/assets/chime.wav" "$ctx/"
    echo "$dockerfile" | podman build "$@" -t "$IMAGE_NAME" -f - "$ctx"
    rm -rf "$ctx"
}
if $REBUILD; then
    info "Rebuilding sandbox image..."
    build_image --no-cache
elif ! podman image exists "$IMAGE_NAME" 2>/dev/null; then
    info "Building sandbox image (one-time)..."
    build_image
fi

# Install chime assets into host ~/.local so they survive the bind-mount
# (the ~/.local/bin mount overrides what the Dockerfile COPYs into the image)
mkdir -p "$HOME/.local/bin" "$HOME/.local/share/sounds"
cp "$SCRIPT_DIR/claude-chime-notify.sh" "$HOME/.local/bin/claude-chime-notify"
chmod +x "$HOME/.local/bin/claude-chime-notify"
cp "$SCRIPT_DIR/assets/chime.wav" "$HOME/.local/share/sounds/chime.wav"

info "Starting sandbox..."

podman run -it --rm \
    --name "$CONTAINER_NAME" \
    --hostname "claude-sandbox" \
    --workdir "$WORKDIR" \
    --user claude \
    --userns=keep-id \
    --security-opt label=disable \
    --security-opt "seccomp=$SCRIPT_DIR/claude-sandbox-seccomp.json" \
    --device /dev/dri \
    --group-add video \
    --cpus=4 \
    --cpu-shares=512 \
    --memory=8g \
    "${mounts[@]}" \
    "${envs[@]}" \
    "$IMAGE_NAME" \
    bash -c 'export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$JAVA_HOME/bin:$PATH" && '"$(
        case "$COMMAND" in
            claude) echo 'claude --dangerously-skip-permissions' ;;
            bash) echo 'exec bash' ;;
        esac
    )"
container_exit=$?

# Attempt to upgrade Claude Code on the host after exiting the sandbox.
# The in-sandbox install is read-only, so upgrades must happen here.
info "Checking for Claude Code updates..."
if command -v claude >/dev/null 2>&1; then
    claude update 2>/dev/null && info "Claude Code updated." || info "Already up to date (or update unavailable)."
else
    warn "claude not found on host PATH; skipping update."
fi

exit $container_exit
