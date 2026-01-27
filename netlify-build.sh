#!/bin/bash

set +euxo pipefail

TOOLCHAIN=1.93.0
rustup default $TOOLCHAIN

cargo install just
just doc
