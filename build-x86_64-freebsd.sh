#!/usr/bin/env bash
set -euo pipefail

# Use wrapper scripts for cross compilation.
PATH="$(pwd):$PATH"
CC_WRAPPER="$(which x86_64-freebsd-cc)"
LINKER_WRAPPER="$(which x86_64-freebsd-linker)"
export CC_x86_64_unknown_freebsd=$CC_WRAPPER
export CLANG_PATH=$CC_WRAPPER
export CARGO_TARGET_X86_64_UNKNOWN_FREEBSD_LINKER=$LINKER_WRAPPER

# Add sysroot to third-party bindgen cross compilation
SYSROOT="$HOME/freebsd/amd64/15.1-RELEASE"
export BINDGEN_EXTRA_CLANG_ARGS="--sysroot=$SYSROOT -isystem $SYSROOT/usr/include"

# Bypass problematic jitterentropy module in aws-lc-sys
export AWS_LC_SYS_NO_JITTER_ENTROPY=1

# Use x86_64-unknown-freebsd build cache to avoid conflicts with other targets.
export CARGO_TARGET_DIR="../target-x86_64-unknown-freebsd"

cargo build \
    --target x86_64-unknown-freebsd \
