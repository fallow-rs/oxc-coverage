#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "${BASH_SOURCE[0]%/*}/.." && pwd)"
NAPI_DIR="$ROOT/crates/oxc_coverage_instrument_napi"
VITEST_DIR="$ROOT/examples/vitest-typescript"

usage() {
  cat <<'USAGE'
Usage: ./scripts/check.sh <profile>
       ./scripts/check.sh --list

Run one repository check or preparation profile. The runner never installs
tools or dependencies. Direct profiles fail when a prerequisite is missing.
USAGE
}

list_profiles() {
  cat <<'PROFILES'
fmt                     primitive    Rust formatting
clippy                  primitive    Strict workspace linting
rust-check              primitive    Workspace compilation
rust-test-fast          primitive    Workspace default tests
rust-test               primitive    Full workspace targets
doc-test                primitive    Workspace documentation tests
rust-doc                primitive    Documentation warnings
ast-api                 aggregate    Experimental AST API tests, lint, and docs
typos                   primitive    Repository spelling
version-sync            primitive    Internal version pins
napi-test               primitive    Native Node binding tests
wasi-shim-test          primitive    Generated WASI shim patch behavior
browser-loader-test     primitive    Browser loader package selection
istanbul-diff-strict    primitive    Strict Istanbul byte-for-byte comparison
istanbul-diff-default   primitive    Default profile documented-delta comparison
istanbul-diff           aggregate    Both coverage profile comparisons
prepare-package-surface preparation  Build generated WASI package artifacts
package-surface         primitive    npm package file surfaces
audit                   primitive    RustSec advisory audit
shear                   primitive    Unused Rust dependencies
inspector-smoke         primitive    Real Node inspector conversion
v8-branch-parity        primitive    V8 branch arm counts against Istanbul
real-world-output       primitive    Production-like emitted behavior
real-world-parity       primitive    Pinned production-bundle coverage parity
compose-real-map-parity primitive    Pinned production-map compose parity
real-world-gates        aggregate    Both pinned real-project parity gates
istanbul-upstream       primitive    Upstream Istanbul runtime cases
vitest-typecheck        primitive    Strict Vitest consumer types
vitest-coverage         primitive    Vitest TypeScript coverage run
vitest-verify           primitive    Vitest TypeScript coverage assertions
actionlint              primitive    GitHub Actions syntax
zizmor                  primitive    GitHub Actions security
commitlint              primitive    Conventional branch commits
self-test               primitive    Verification runner regression tests
rust                    aggregate    Rust format, lint, tests, and docs
pre-push                aggregate    Fast hook checks with optional typos and tests
all-local               aggregate    Every host-reproducible verification profile
PROFILES
}

die() {
  echo "check: $1" >&2
  exit 1
}

require_tool() {
  local tool="$1"
  local install_hint="$2"
  if ! command -v "$tool" >/dev/null 2>&1; then
    die "Required tool '$tool' is not installed. $install_hint"
  fi
}

require_file() {
  local path="$1"
  local prepare_hint="$2"
  if [ ! -f "$path" ]; then
    die "Required artifact '${path#"$ROOT"/}' is missing. $prepare_hint"
  fi
}

require_dir() {
  local path="$1"
  local prepare_hint="$2"
  if [ ! -d "$path" ]; then
    die "Required directory '${path#"$ROOT"/}' is missing. $prepare_hint"
  fi
}

require_node_22() {
  local profile="$1"
  require_tool node "Install Node.js 22."
  local major
  major="$(node -p 'process.versions.node.split(".")[0]')"
  if [ "$major" != "22" ]; then
    die "Node.js 22 is required for $profile. Found major version $major."
  fi
}

require_python_311() {
  require_tool python3 "Install Python 3.11 or newer."
  local version
  if ! version="$(python3 -c '
import sys
print(f"{sys.version_info.major}.{sys.version_info.minor}")
import tomllib
raise SystemExit(sys.version_info < (3, 11))
' 2>/dev/null)"; then
    die "Python 3.11 or newer with tomllib is required for version-sync. Found ${version:-unknown}."
  fi
}

NAPI_BUILD_HINT="Run 'npm --prefix crates/oxc_coverage_instrument_napi run build:debug'."

# Print the first Rust input of the binding that is newer than the artifact.
# The CLI crate is not a dependency of the binding, so its sources are excluded.
newer_napi_input() {
  local artifact="$1"
  find "$ROOT/Cargo.toml" "$ROOT/Cargo.lock" "$ROOT/crates" \
    \( -path "$ROOT/crates/oxc_coverage_instrument_cli" -o -name node_modules -o -name target \) \
    -prune -o \
    \( -path "$ROOT/crates/*/src/*" -name '*.rs' \
    -o -path "$ROOT/crates/*/build.rs" \
    -o -path "$ROOT/crates/*/Cargo.toml" \
    -o -path "$ROOT/Cargo.toml" \
    -o -path "$ROOT/Cargo.lock" \) \
    -newer "$artifact" -print -quit 2>/dev/null || true
}

require_napi_artifact() {
  local artifact
  local found=0
  local newer
  for artifact in "$NAPI_DIR"/coverage-instrument.*.node "$NAPI_DIR"/coverage-instrument.node; do
    if [ ! -f "$artifact" ]; then
      continue
    fi
    found=1
    newer="$(newer_napi_input "$artifact")"
    if [ -n "$newer" ]; then
      die "The native N-API artifact '${artifact#"$ROOT"/}' is older than '${newer#"$ROOT"/}'. $NAPI_BUILD_HINT"
    fi
  done
  if [ "$found" -eq 0 ]; then
    die "A native N-API artifact is missing. $NAPI_BUILD_HINT"
  fi
}

run_fmt() {
  require_tool cargo "Install Rust using rustup."
  echo "[check:fmt] cargo fmt --all --check"
  cargo fmt --all --check
}

run_clippy() {
  require_tool cargo "Install Rust using rustup, including the clippy component."
  echo "[check:clippy] cargo clippy --workspace --all-targets -- -D warnings"
  cargo clippy --workspace --all-targets -- -D warnings
}

run_rust_check() {
  require_tool cargo "Install Rust using rustup."
  echo "[check:rust-check] cargo check --workspace"
  cargo check --workspace
}

run_rust_test_fast() {
  require_tool cargo "Install Rust using rustup."
  echo "[check:rust-test-fast] cargo test --workspace"
  cargo test --workspace
}

run_rust_test() {
  require_tool cargo "Install Rust using rustup."
  echo "[check:rust-test] cargo test --workspace --all-targets"
  cargo test --workspace --all-targets
}

run_doc_test() {
  require_tool cargo "Install Rust using rustup."
  echo "[check:doc-test] cargo test --workspace --doc"
  cargo test --workspace --doc
}

run_rust_doc() {
  require_tool cargo "Install Rust using rustup."
  echo "[check:rust-doc] RUSTDOCFLAGS='-D warnings' cargo doc --no-deps --document-private-items"
  RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --document-private-items
}

run_ast_api() {
  require_tool cargo "Install Rust using rustup, including the clippy component."
  echo "[check:ast-api] cargo test -p oxc_coverage_instrument --features ast-api --all-targets"
  cargo test -p oxc_coverage_instrument --features ast-api --all-targets
  echo "[check:ast-api] cargo test -p oxc_coverage_instrument --features ast-api --doc"
  cargo test -p oxc_coverage_instrument --features ast-api --doc
  echo "[check:ast-api] cargo clippy -p oxc_coverage_instrument --features ast-api --all-targets -- -D warnings"
  cargo clippy -p oxc_coverage_instrument --features ast-api --all-targets -- -D warnings
  echo "[check:ast-api] RUSTDOCFLAGS='-D warnings' cargo doc -p oxc_coverage_instrument --features ast-api --no-deps --document-private-items"
  RUSTDOCFLAGS="-D warnings" cargo doc -p oxc_coverage_instrument --features ast-api --no-deps --document-private-items
}

run_typos() {
  local optional="${1:-0}"
  if ! command -v typos >/dev/null 2>&1; then
    if [ "$optional" = "1" ]; then
      echo "[check:typos] typos is not installed; skipping optional pre-push check"
      echo "[check:typos] install with: cargo install typos-cli"
      return
    fi
    die "Required tool 'typos' is not installed. Install with: cargo install typos-cli"
  fi
  if [ "$optional" = "1" ]; then
    echo "[check:typos] typos ."
    typos .
  else
    echo "[check:typos] typos"
    typos
  fi
}

run_version_sync() {
  require_python_311
  require_tool node "Install Node.js 22."
  echo "[check:version-sync] ./scripts/check-version-sync.sh --mode=pins"
  ./scripts/check-version-sync.sh --mode=pins
  echo "[check:version-sync] node scripts/check-version-sync.test.mjs"
  node scripts/check-version-sync.test.mjs
  echo "[check:version-sync] node scripts/release-workflow-policy.test.mjs"
  node scripts/release-workflow-policy.test.mjs
  echo "[check:version-sync] node scripts/npm-pack-surface-check.mjs --metadata-only"
  node scripts/npm-pack-surface-check.mjs --metadata-only
}

require_native_napi() {
  require_tool node "Install Node.js 22."
  require_file "$NAPI_DIR/index.js" "Build the N-API package first."
  require_napi_artifact
}

run_napi_test() {
  require_native_napi
  echo "[check:napi-test] node crates/oxc_coverage_instrument_napi/test.mjs"
  node crates/oxc_coverage_instrument_napi/test.mjs
}

run_wasi_shim_test() {
  require_tool node "Install Node.js 22."
  echo "[check:wasi-shim-test] (cd crates/oxc_coverage_instrument_napi && node scripts/patch-wasi-browser-shim.test.mjs)"
  (cd "$NAPI_DIR" && node scripts/patch-wasi-browser-shim.test.mjs)
}

run_browser_loader_test() {
  require_tool npm "Install npm with Node.js."
  require_dir "$NAPI_DIR/node_modules" "Run 'npm --prefix crates/oxc_coverage_instrument_napi install'."
  require_file "$NAPI_DIR/browser.js" "Build and patch the N-API package first."
  echo "[check:browser-loader-test] (cd crates/oxc_coverage_instrument_napi && npm run test:browser-loader)"
  (cd "$NAPI_DIR" && npm run test:browser-loader)
}

run_istanbul_diff_profile() {
  local profile="$1"
  require_native_napi
  require_dir "$ROOT/node_modules/istanbul-lib-instrument" "Run 'npm install' at the repository root."
  echo "[check:istanbul-diff-$profile] node scripts/istanbul-diff.mjs --profile=$profile"
  node scripts/istanbul-diff.mjs "--profile=$profile"
}

run_istanbul_diff() {
  run_istanbul_diff_profile strict
  run_istanbul_diff_profile default
}

run_prepare_package_surface() {
  echo "[check:prepare-package-surface] ./scripts/prepare-package-surface.sh"
  ./scripts/prepare-package-surface.sh
}

run_package_surface() {
  require_tool node "Install Node.js 22."
  require_tool npm "Install npm with Node.js."
  require_file "$NAPI_DIR/index.js" "Build and prepare the N-API packages first."
  require_file "$NAPI_DIR/npm/wasm32-wasi/coverage-instrument.wasm32-wasi.wasm" "Run './scripts/check.sh prepare-package-surface' first."
  require_file "$NAPI_DIR/npm/wasm32-wasi-singlethreaded/coverage-instrument.wasm32-wasi.wasm" "Run './scripts/check.sh prepare-package-surface' first."
  echo "[check:package-surface] node scripts/npm-pack-surface-check.mjs"
  node scripts/npm-pack-surface-check.mjs
}

run_audit() {
  require_tool cargo "Install Rust using rustup."
  require_tool cargo-audit "Install with: cargo install cargo-audit"
  echo "[check:audit] cargo audit"
  cargo audit
}

run_shear() {
  require_tool cargo "Install Rust using rustup."
  require_tool cargo-shear "Install with: cargo install cargo-shear"
  echo "[check:shear] cargo shear"
  cargo shear
}

run_inspector_smoke() {
  require_node_22 inspector-smoke
  require_native_napi
  echo "[check:inspector-smoke] node scripts/v8-inspector-smoke.mjs"
  node scripts/v8-inspector-smoke.mjs
}

run_v8_branch_parity() {
  require_node_22 v8-branch-parity
  require_native_napi
  require_dir "$ROOT/node_modules/istanbul-lib-instrument" "Run 'npm install' at the repository root."
  echo "[check:v8-branch-parity] node scripts/v8-branch-parity.mjs"
  node scripts/v8-branch-parity.mjs
}

run_real_world_output() {
  require_native_napi
  echo "[check:real-world-output] node scripts/real-world-output.mjs"
  node scripts/real-world-output.mjs
}

require_real_world_corpus() {
  require_tool node "Install Node.js 22."
  require_dir "$ROOT/node_modules/istanbul-lib-instrument" "Run 'npm install' at the repository root."
  require_native_napi
  echo "[check:real-world-corpus] node scripts/prepare-real-world-corpus.mjs --check-only"
  node scripts/prepare-real-world-corpus.mjs --check-only
}

run_real_world_parity() {
  require_real_world_corpus
  echo "[check:real-world-parity] node scripts/real-world-parity.mjs"
  node scripts/real-world-parity.mjs
}

run_compose_real_map_parity() {
  require_real_world_corpus
  echo "[check:compose-real-map-parity] node scripts/compose-real-map-parity.mjs"
  node scripts/compose-real-map-parity.mjs
}

run_real_world_gates() {
  run_real_world_parity
  run_compose_real_map_parity
}

run_istanbul_upstream() {
  require_native_napi
  echo "[check:istanbul-upstream] node scripts/istanbul-upstream-specs.mjs"
  node scripts/istanbul-upstream-specs.mjs
}

require_vitest_example() {
  require_tool npm "Install npm with Node.js."
  require_dir "$VITEST_DIR/node_modules" "Run 'npm --prefix examples/vitest-typescript install'."
}

run_vitest_typecheck() {
  require_vitest_example
  echo "[check:vitest-typecheck] (cd examples/vitest-typescript && npm run typecheck)"
  (cd "$VITEST_DIR" && npm run typecheck)
}

run_vitest_coverage() {
  require_vitest_example
  require_native_napi
  echo "[check:vitest-coverage] (cd examples/vitest-typescript && npm run coverage)"
  (cd "$VITEST_DIR" && npm run coverage)
}

run_vitest_verify() {
  require_vitest_example
  require_file "$VITEST_DIR/coverage/coverage-final.json" "Run the vitest-coverage profile first."
  echo "[check:vitest-verify] (cd examples/vitest-typescript && npm run verify)"
  (cd "$VITEST_DIR" && npm run verify)
}

run_actionlint() {
  require_tool actionlint "Install actionlint using Homebrew or its documented release binary."
  echo "[check:actionlint] actionlint .github/workflows/*.yml"
  actionlint .github/workflows/*.yml
}

# Single source of truth for the zizmor version, read from the CI step so local
# runs cannot drift from what the Actions Security job actually enforces. A
# floating `uvx zizmor` in CI plus whatever zizmor a developer happened to
# install locally is how a locally-green workflow change lands red on main.
zizmor_pinned_version() {
  sed -n 's/.*uvx zizmor@\([0-9][0-9.]*\).*/\1/p' "$ROOT/.github/workflows/ci.yml" | head -1
}

run_zizmor() {
  local pinned
  pinned="$(zizmor_pinned_version)"
  if [ -z "$pinned" ]; then
    echo "[check:zizmor] could not read the pinned zizmor version from .github/workflows/ci.yml" >&2
    return 1
  fi

  local args=(--config .github/zizmor.yml --min-confidence medium --format plain .github/)

  # Prefer uvx so the local run resolves the identical build CI resolves.
  if command -v uvx >/dev/null 2>&1; then
    echo "[check:zizmor] uvx zizmor@${pinned} ${args[*]}"
    uvx "zizmor@${pinned}" "${args[@]}"
    return
  fi

  require_tool zizmor "Install uv (preferred, matches CI exactly) or zizmor ${pinned} via Homebrew, cargo, or pip."
  local installed
  installed="$(zizmor --version 2>/dev/null | awk '{print $2}')"
  if [ "$installed" != "$pinned" ]; then
    echo "[check:zizmor] WARNING: local zizmor ${installed:-unknown} != CI-pinned ${pinned}." >&2
    echo "[check:zizmor] A pass here does not guarantee the Actions Security job passes." >&2
    echo "[check:zizmor] Install uv so this target runs the same build as CI." >&2
  fi
  echo "[check:zizmor] zizmor ${args[*]}"
  zizmor "${args[@]}"
}

run_commitlint() {
  require_tool npm "Install npm with Node.js."
  require_tool git "Install Git."
  require_dir "$ROOT/node_modules/@commitlint/cli" "Run 'npm install' at the repository root."
  if ! git show-ref --verify --quiet refs/remotes/origin/main; then
    die "Required ref 'origin/main' is missing. Fetch origin/main before running commitlint."
  fi
  echo "[check:commitlint] npm run commitlint -- --from origin/main --to HEAD"
  npm run commitlint -- --from origin/main --to HEAD
}

run_self_test() {
  echo "[check:self-test] ./scripts/check-self-test.sh"
  ./scripts/check-self-test.sh
}

run_rust() {
  run_fmt
  run_clippy
  run_rust_test
  run_doc_test
  run_rust_doc
}

run_pre_push() {
  run_fmt
  run_clippy
  run_typos 1
  if [ "${RUN_TESTS:-0}" = "1" ]; then
    require_tool cargo "Install Rust using rustup."
    echo "[check:pre-push] cargo test --workspace"
    cargo test --workspace
  fi
}

print_ci_only() {
  cat <<'CI_ONLY'
[check:all-local] Not run locally by this profile:
  CI-only: OS and target matrices, MSRV, clean-worktree validation, WASM builds and tests, native-vs-WASM parity, example platform smokes, coverage generation and upload, release publication
  Action-owned: crate-ci typos and cargo-deny-action
  Workflow-owned: artifact uploads, caches, permissions, job aggregation, commit messages, and issue-link policy
CI_ONLY
}

run_all_local() {
  run_self_test
  run_rust
  run_ast_api
  run_rust_check
  run_rust_test_fast
  run_typos
  run_version_sync
  run_napi_test
  run_wasi_shim_test
  run_browser_loader_test
  run_istanbul_diff
  run_package_surface
  run_audit
  run_shear
  run_inspector_smoke
  run_v8_branch_parity
  run_real_world_output
  run_real_world_gates
  run_istanbul_upstream
  run_vitest_typecheck
  run_vitest_coverage
  run_vitest_verify
  run_actionlint
  run_zizmor
  run_commitlint
  print_ci_only
}

cd "$ROOT"

profile="${1:-}"
case "$profile" in
  -h|--help) usage ;;
  --list) list_profiles ;;
  fmt) run_fmt ;;
  clippy) run_clippy ;;
  rust-check) run_rust_check ;;
  rust-test-fast) run_rust_test_fast ;;
  rust-test) run_rust_test ;;
  doc-test) run_doc_test ;;
  rust-doc) run_rust_doc ;;
  ast-api) run_ast_api ;;
  typos) run_typos ;;
  version-sync) run_version_sync ;;
  napi-test) run_napi_test ;;
  wasi-shim-test) run_wasi_shim_test ;;
  browser-loader-test) run_browser_loader_test ;;
  istanbul-diff-strict) run_istanbul_diff_profile strict ;;
  istanbul-diff-default) run_istanbul_diff_profile default ;;
  istanbul-diff) run_istanbul_diff ;;
  prepare-package-surface) run_prepare_package_surface ;;
  package-surface) run_package_surface ;;
  audit) run_audit ;;
  shear) run_shear ;;
  inspector-smoke) run_inspector_smoke ;;
  v8-branch-parity) run_v8_branch_parity ;;
  real-world-output) run_real_world_output ;;
  real-world-parity) run_real_world_parity ;;
  compose-real-map-parity) run_compose_real_map_parity ;;
  real-world-gates) run_real_world_gates ;;
  istanbul-upstream) run_istanbul_upstream ;;
  vitest-typecheck) run_vitest_typecheck ;;
  vitest-coverage) run_vitest_coverage ;;
  vitest-verify) run_vitest_verify ;;
  actionlint) run_actionlint ;;
  zizmor) run_zizmor ;;
  commitlint) run_commitlint ;;
  self-test) run_self_test ;;
  rust) run_rust ;;
  pre-push) run_pre_push ;;
  all-local) run_all_local ;;
  "") usage >&2; exit 2 ;;
  *) echo "Unknown profile: $profile" >&2; usage >&2; exit 2 ;;
esac
