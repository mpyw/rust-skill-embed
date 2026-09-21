#!/usr/bin/env bash

set -o pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m'

# The toolchain is pinned in rust-toolchain.toml, so a local checkout and CI
# run the same rustc, clippy and rustfmt.

declare -a failed_tests=()

run_test() {
    local name="$1"
    shift

    echo "=== $name ==="
    "$@"
    local status=$?
    if [ $status -eq 0 ]; then
        echo -e "${GREEN}[$name] OK${NC}"
    else
        echo -e "${RED}[$name] FAILED${NC}"
        failed_tests+=("$name")
    fi
    return $status
}

echo ""

# --workspace reaches the adapter, the two examples and the crate that compiles
# README.md, which are separate crates so that embedding skills never drags
# clap into a consumer's dependency graph.
run_test "test" \
    cargo test --workspace --all-features

# The core has to build without include_dir, because a tool embedding its
# skills some other way turns that feature off.
run_test "no-default-features" \
    cargo check -p skill-embed --no-default-features

run_test "fmt" \
    cargo fmt --all --check

run_test "clippy" \
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# A `cfg` block is always compiled on the platform a contributor is on, so a
# lint that fires only elsewhere ships. An unused import behind `cfg(unix)` went
# out this way and came back from the Windows job. One other target catches it
# first. Skipped when that target is not installed, since CI covers all three.
run_test "cross-clippy" \
    bash -c '
      case "$(uname -s)" in
        MINGW*|MSYS*|CYGWIN*) target=x86_64-unknown-linux-gnu ;;
        *)                    target=x86_64-pc-windows-msvc ;;
      esac
      if ! rustup target list --installed | grep -qx "$target"; then
        echo "skipped: run \"rustup target add $target\" to check it here"
        exit 0
      fi
      cargo clippy --workspace --all-targets --all-features --target "$target" -- -D warnings
    '

# A broken intra-doc link is not a build failure, so it needs asking for.
run_test "doc" \
    env RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features

echo ""
echo "===== Summary ====="
if [ ${#failed_tests[@]} -eq 0 ]; then
    echo -e "${GREEN}All tests passed!${NC}"
    exit 0
else
    echo -e "${RED}Failed tests:${NC}"
    for test in "${failed_tests[@]}"; do
        echo -e "  ${RED}- $test${NC}"
    done
    exit 1
fi
