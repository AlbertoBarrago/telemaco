#!/usr/bin/env bash
# Reproduce the CI "core" job locally.
#
# Runs the same build and test matrix .github/workflows/ci.yml runs on a pull
# request, minus the Ubuntu package install. Use it before pushing so a red CI
# is not the first time you learn a feature combination is broken.
#
#   scripts/ci/local.sh            # every step
#   scripts/ci/local.sh --quick    # skip the two stealth steps (no CMake needed)
#
# The stealth steps build BoringSSL through CMake, so `cmake` must be on PATH.
set -uo pipefail

cd "$(dirname "$0")/../.."

QUICK=0
[[ "${1:-}" == "--quick" ]] && QUICK=1

export CARGO_INCREMENTAL=0
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
export CARGO_TERM_COLOR=always

FAILED=()
step() {
  local name="$1"; shift
  printf '\n\033[1m── %s\033[0m\n' "$name"
  if "$@" >/tmp/ci-local-step.log 2>&1; then
    printf '\033[32mPASS\033[0m  %s\n' "$name"
  else
    printf '\033[31mFAIL\033[0m  %s\n' "$name"
    tail -25 /tmp/ci-local-step.log | sed 's/^/    /'
    FAILED+=("$name")
  fi
}

if [[ $QUICK -eq 0 ]] && ! command -v cmake >/dev/null; then
  echo "cmake not found: the stealth steps need it. Re-run with --quick to skip them." >&2
  exit 2
fi

step "build render release" \
  cargo build --release -p telemaco-cli --bins --features render
step "test render configuration" \
  cargo nextest run --release --features render --no-fail-fast

if [[ $QUICK -eq 0 ]]; then
  step "build render and stealth release" \
    cargo build --release -p telemaco-cli --bins --features render,stealth
  # One test needs a loopback fixture, so it runs separately with the SSRF gate
  # opened; the rest must pass with the gate closed, as CI runs them.
  step "test the stealth transport" \
    cargo nextest run --release -p telemaco-net --features stealth \
      --no-fail-fast -- --skip stealth_client_decodes_gzip_response
  # `env` rather than a prefix assignment: in bash a VAR=x prefix on a shell
  # function has murky scoping, and this one must reach cargo.
  step "test the stealth gzip path" \
    env TELEMACO_ALLOW_PRIVATE_NETWORK=1 \
      cargo nextest run --release -p telemaco-net --features stealth \
        stealth_client_decodes_gzip_response --no-fail-fast
fi

step "build no-render release" \
  cargo build --release -p telemaco-cli --bins --no-default-features
# `telemaco` keeps its default `api` feature: an empty feature set is not a
# supported configuration for the library crate.
step "test no-render: library" \
  cargo nextest run --release -p telemaco --no-fail-fast
step "test no-render: workspace" \
  cargo nextest run --release --workspace --exclude telemaco \
    --exclude telemaco-render --no-default-features --no-fail-fast
# telemaco-render has paint-only fixtures, so check the library without them.
step "check no-render: render crate" \
  cargo check --release -p telemaco-render --no-default-features

if [[ $QUICK -eq 0 ]]; then
  step "build no-render and stealth release" \
    cargo build --release -p telemaco-cli --bins --no-default-features --features stealth
fi

printf '\n'
if [[ ${#FAILED[@]} -eq 0 ]]; then
  printf '\033[32mall steps passed\033[0m\n'
else
  printf '\033[31m%d step(s) failed:\033[0m\n' "${#FAILED[@]}"
  printf '  %s\n' "${FAILED[@]}"
  exit 1
fi
