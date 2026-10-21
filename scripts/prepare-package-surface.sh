#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "${BASH_SOURCE[0]%/*}/.." && pwd)"
NAPI_DIR="$ROOT/crates/oxc_coverage_instrument_napi"
THREADED_PACKAGE="$NAPI_DIR/npm/wasm32-wasi"
SINGLE_THREADED_PACKAGE="$NAPI_DIR/npm/wasm32-wasi-singlethreaded"
TMP="$(mktemp -d)"
COMMITTED=0
SNAPSHOT_READY=0

die() {
  echo "prepare-package-surface: $1" >&2
  exit 1
}

require_tool() {
  local tool="$1"
  local hint="$2"
  if ! command -v "$tool" >/dev/null 2>&1; then
    die "Required tool '$tool' is not installed. $hint"
  fi
}

package_files() {
  local manifest="$1"
  node -e '
    const manifest = require(process.argv[1]);
    for (const file of manifest.files) console.log(file);
  ' "$manifest"
}

copy_package_files() {
  local source_dir="$1"
  local target_dir="$2"
  local manifest="$3"
  local file
  mkdir -p "$target_dir"
  while IFS= read -r file; do
    if [ ! -f "$source_dir/$file" ]; then
      die "WASI build did not produce '$file' in ${source_dir#"$ROOT"/}."
    fi
    cp "$source_dir/$file" "$target_dir/$file"
  done < <(package_files "$manifest")
}

snapshot_manifest_files() {
  local source_dir="$1"
  local snapshot_dir="$2"
  local manifest="$3"
  local file
  mkdir -p "$snapshot_dir"
  while IFS= read -r file; do
    if [ -f "$source_dir/$file" ]; then
      cp "$source_dir/$file" "$snapshot_dir/$file"
    fi
  done < <(package_files "$manifest")
}

restore_manifest_files() {
  local target_dir="$1"
  local snapshot_dir="$2"
  local manifest="$3"
  local file
  local files
  local status=0
  if ! files="$(package_files "$manifest")"; then
    return 1
  fi
  while IFS= read -r file; do
    if [ -z "$file" ]; then
      continue
    fi
    if ! rm -f "$target_dir/$file"; then
      status=1
    fi
    if [ -f "$snapshot_dir/$file" ]; then
      if ! cp "$snapshot_dir/$file" "$target_dir/$file"; then
        status=1
      fi
    fi
  done <<<"$files"
  return "$status"
}

snapshot_root_artifacts() {
  local snapshot_dir="$1"
  local artifact
  snapshot_manifest_files "$NAPI_DIR" "$snapshot_dir" "$NAPI_DIR/package.json"
  snapshot_manifest_files "$NAPI_DIR" "$snapshot_dir" "$THREADED_PACKAGE/package.json"
  for artifact in "$NAPI_DIR"/coverage-instrument.wasm32-wasi*.wasm; do
    if [ -f "$artifact" ]; then
      cp "$artifact" "$snapshot_dir/"
    fi
  done
}

restore_root_artifacts() {
  local snapshot_dir="$1"
  local artifact
  local status=0
  if ! restore_manifest_files "$NAPI_DIR" "$snapshot_dir" "$NAPI_DIR/package.json"; then
    status=1
  fi
  if ! restore_manifest_files "$NAPI_DIR" "$snapshot_dir" "$THREADED_PACKAGE/package.json"; then
    status=1
  fi
  if ! rm -f "$NAPI_DIR"/coverage-instrument.wasm32-wasi*.wasm; then
    status=1
  fi
  for artifact in "$snapshot_dir"/coverage-instrument.wasm32-wasi*.wasm; do
    if [ -f "$artifact" ]; then
      if ! cp "$artifact" "$NAPI_DIR/"; then
        status=1
      fi
    fi
  done
  return "$status"
}

snapshot_destinations() {
  snapshot_root_artifacts "$TMP/original-root"
  snapshot_manifest_files \
    "$THREADED_PACKAGE" "$TMP/original-threaded" "$THREADED_PACKAGE/package.json"
  snapshot_manifest_files \
    "$SINGLE_THREADED_PACKAGE" \
    "$TMP/original-single-threaded" \
    "$SINGLE_THREADED_PACKAGE/package.json"
}

restore_destinations() {
  local status=0
  if ! restore_root_artifacts "$TMP/original-root"; then
    status=1
  fi
  if ! restore_manifest_files \
    "$THREADED_PACKAGE" "$TMP/original-threaded" "$THREADED_PACKAGE/package.json"; then
    status=1
  fi
  if ! restore_manifest_files \
    "$SINGLE_THREADED_PACKAGE" \
    "$TMP/original-single-threaded" \
    "$SINGLE_THREADED_PACKAGE/package.json"; then
    status=1
  fi
  return "$status"
}

cleanup() {
  local status=$?
  local restore_failed=0
  trap - EXIT
  if [ "$SNAPSHOT_READY" = "1" ] && [ "$COMMITTED" != "1" ] && ! restore_destinations; then
    echo "prepare-package-surface: failed to restore original artifacts" >&2
    echo "prepare-package-surface: recovery snapshot retained at $TMP" >&2
    restore_failed=1
    status=1
  fi
  if [ "$restore_failed" != "1" ]; then
    rm -rf "$TMP"
  fi
  exit "$status"
}
trap cleanup EXIT

require_tool node "Install Node.js 22."
require_tool npm "Install npm with Node.js."
require_tool npx "Install npm with Node.js."
require_tool cargo "Install Rust using rustup."
require_tool rustup "Install Rust using rustup."

if [ ! -d "$NAPI_DIR/node_modules/@napi-rs/cli" ]; then
  die "N-API dependencies are missing. Run 'npm --prefix crates/oxc_coverage_instrument_napi install'."
fi

installed_targets="$(rustup target list --installed)"
for target in wasm32-wasip1-threads wasm32-wasip1; do
  if ! grep -Fxq "$target" <<<"$installed_targets"; then
    die "Rust target '$target' is missing. Run 'rustup target add $target'."
  fi
done

snapshot_destinations
SNAPSHOT_READY=1

echo "[prepare:package-surface] build threaded wasm32-wasip1-threads artifacts"
(
  cd "$NAPI_DIR"
  rm -f coverage-instrument.wasm32-wasi*.wasm
  npx napi build --release --platform --target wasm32-wasip1-threads
  node scripts/patch-wasi-emnapi-plugins.mjs .
  node scripts/patch-wasi-browser-shim.mjs
)
snapshot_root_artifacts "$TMP/threaded-root"

echo "[prepare:package-surface] build single-threaded wasm32-wasip1 artifacts"
(
  cd "$NAPI_DIR"
  rm -f coverage-instrument.wasm32-wasi*.wasm
  npx napi build --release --platform --target wasm32-wasip1
  node scripts/patch-wasi-emnapi-plugins.mjs .
  node scripts/prepare-wasi-singlethreaded-package.mjs .
  node scripts/validate-wasi-singlethreaded-package.mjs npm/wasm32-wasi-singlethreaded
)

copy_package_files "$TMP/threaded-root" "$THREADED_PACKAGE" "$THREADED_PACKAGE/package.json"
restore_root_artifacts "$TMP/threaded-root"
(cd "$NAPI_DIR" && node scripts/patch-browser-loader.mjs)
COMMITTED=1

echo "[prepare:package-surface] package artifacts are ready"
