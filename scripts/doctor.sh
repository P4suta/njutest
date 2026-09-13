#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 njutest contributors
# SPDX-License-Identifier: MIT OR Apache-2.0
#
# Report the development tools this repository expects, read-only. The pins it
# compares against are rust-toolchain.toml and mise.toml; nothing here installs
# anything.
set -uo pipefail

status=0
ok()   { printf '  \033[32m✓\033[0m %s\n' "$1"; }
bad()  { printf '  \033[31m✗\033[0m %s\n' "$1"; status=1; }
warn() { printf '  \033[33m!\033[0m %s\n' "$1"; }

pinned_channel=$(sed -n 's/^channel *= *"\(.*\)"/\1/p' rust-toolchain.toml)
echo "toolchain"
if command -v rustc >/dev/null 2>&1; then
    version=$(rustc --version | awk '{print $2}')
    if [ "$version" = "$pinned_channel" ]; then
        ok "rustc $version matches rust-toolchain.toml"
    else
        bad "rustc $version but rust-toolchain.toml pins $pinned_channel"
    fi
else
    bad "rustc not found"
fi
sysroot=$(rustc --print sysroot 2>/dev/null || true)
host=$(rustc -vV 2>/dev/null | sed -n 's/^host: //p')
for tool in llvm-profdata llvm-cov; do
    if [ -n "$sysroot" ] && [ -x "$sysroot/lib/rustlib/$host/bin/$tool" ]; then
        ok "$tool (llvm-tools component)"
    else
        bad "$tool missing: rustup component add llvm-tools"
    fi
done
if rustup run nightly rustc --version >/dev/null 2>&1; then
    ok "nightly toolchain ($(rustup run nightly rustc --version | awk '{print $2}')) — cargo-fuzz and Miri are available"
else
    warn "no nightly toolchain: cargo-fuzz and Miri phases will be reported as limitations"
fi

echo "cargo tools"
for tool in cargo-nextest cargo-deny cargo-llvm-cov cargo-audit cargo-mutants cargo-fuzz; do
    if command -v "$tool" >/dev/null 2>&1; then
        ok "$tool"
    else
        warn "$tool not found (mise install, or optional for fuzz/miri)"
    fi
done
# rustup puts a `cargo-miri` shim on the path whether or not the component is
# installed, and the shim refuses when it is not. Ask it, rather than the path.
if cargo +nightly miri --version >/dev/null 2>&1; then
    ok "cargo-miri"
else
    warn "cargo-miri not installed for nightly: the deep-v1 contract will fail closed with NJ7001 (rustup +nightly component add miri)"
fi

echo "repository tools"
for tool in mise lefthook committed typos taplo actionlint mdbook git; do
    if command -v "$tool" >/dev/null 2>&1; then
        ok "$tool"
    else
        bad "$tool not found: mise install"
    fi
done

echo "git hooks"
if [ -f .git/hooks/pre-commit ] && grep -q lefthook .git/hooks/pre-commit 2>/dev/null; then
    ok "lefthook hooks installed"
else
    warn "lefthook hooks not installed: lefthook install"
fi

exit $status
