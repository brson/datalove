#!/bin/bash

set -euxo pipefail

cd "$(dirname "$0")/.."

TOOLCHAIN=1.95.0
rustup default $TOOLCHAIN

cargo install just --locked
just doc
