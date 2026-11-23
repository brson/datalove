#!/bin/bash

set -euxo pipefail

cd "$(dirname "$0")/.."

TOOLCHAIN=1.95.0
rustup default $TOOLCHAIN

cargo install just --locked
# Netlify sets CONTEXT to "production" only for the main site deploy,
# not for deploy previews or branch deploys.
if [ "${CONTEXT:-}" = production ]; then
    just doc-production
else
    just doc
fi
