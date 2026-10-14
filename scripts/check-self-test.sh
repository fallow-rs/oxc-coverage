#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CHECK="$ROOT/scripts/check.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

fail() {
  echo "check self-test failed: $1" >&2
  exit 1
}

assert_contains() {
  local file="$1"
  local expected="$2"
  if ! grep -Fq -- "$expected" "$file"; then
    echo "expected output to contain: $expected" >&2
    sed -n '1,120p' "$file" >&2
    fail "unexpected output"
  fi
}

assert_equal() {
  local actual="$1"
  local expected="$2"
  local context="$3"
  if [ "$actual" != "$expected" ]; then
    echo "expected $context:" >&2
    printf '%s\n' "$expected" >&2
    echo "actual $context:" >&2
    printf '%s\n' "$actual" >&2
    fail "$context differs"
  fi
}

assert_file_equal() {
  local file="$1"
  local expected="$2"
  local context="$3"
  if [ ! -f "$file" ]; then
    fail "$context is missing"
  fi
  assert_equal "$(<"$file")" "$expected" "$context"
}

assert_file_differs() {
  if cmp -s "$1" "$2"; then
    fail "$3 fixture did not change the workflow"
  fi
}

workflow_check_has_linux_profile_step() {
  local workflow="$1"
  local step_name="$2"
  local profile="$3"

  awk -v target_name="      - name: $step_name" -v target_run="        run: ./scripts/check.sh $profile" '
    function finish_step() {
      if (in_target && has_if && has_run) found = 1
    }
    $0 == "  check:" { in_job = 1; next }
    in_job && $0 ~ /^  [[:alnum:]_-]+:$/ { finish_step(); exit }
    in_job && $0 ~ /^      - / {
      finish_step()
      in_target = ($0 == target_name)
      has_if = 0
      has_run = 0
      next
    }
    in_target && $0 == "        if: runner.os == '\''Linux'\''" { has_if = 1 }
    in_target && $0 == target_run { has_run = 1 }
    END { finish_step(); exit(found ? 0 : 1) }
  ' "$workflow"
}

workflow_real_world_job_is_gated() {
  local workflow="$1"

  awk '
    $0 == "  real-world-parity:" { in_job = 1; next }
    in_job && $0 ~ /^  [[:alnum:]_-]+:$/ { exit }
    in_job && /hashFiles\('\''scripts\/real-world-corpus\.json'\''\)/ { has_cache_key = 1 }
    in_job && $0 == "        run: node scripts/prepare-real-world-corpus.mjs" {
      has_prepare = 1
    }
    in_job && $0 == "        run: ./scripts/check.sh real-world-gates" { has_gate = 1 }
    END { exit(has_cache_key && has_prepare && has_gate ? 0 : 1) }
  ' "$workflow"
}

workflow_version_sync_has_node_and_profile() {
  local workflow="$1"

  awk '
    function finish_step() {
      if (in_setup_node && has_node_version) {
        found_setup_node = 1
      }
    }

    $0 == "  version-sync:" {
      in_job = 1
      next
    }
    in_job && $0 ~ /^  [[:alnum:]_-]+:$/ {
      finish_step()
      exit
    }
    in_job && $0 ~ /^      - / {
      finish_step()
      in_setup_node = ($0 ~ /^      - uses: actions\/setup-node@/)
      has_node_version = 0
      if ($0 == "      - run: ./scripts/check.sh version-sync") {
        found_profile = 1
      }
      next
    }
    in_setup_node && $0 == "          node-version: 22" { has_node_version = 1 }

    END {
      finish_step()
      exit(found_setup_node && found_profile ? 0 : 1)
    }
  ' "$workflow"
}

workflow_job_has_need() {
  local workflow="$1"
  local job="$2"
  local dependency="$3"

  awk -v job="$job" -v dependency="$dependency" '
    $0 == "  " job ":" {
      in_job = 1
      next
    }
    in_job && $0 ~ /^  [[:alnum:]_-]+:$/ { exit }
    in_job && $0 == "    needs: " dependency { found = 1 }
    in_job && $0 == "    needs:" {
      in_needs = 1
      next
    }
    in_needs && $0 ~ /^    [^ ]/ { in_needs = 0 }
    in_needs && $0 == "      - " dependency { found = 1 }
    END { exit(found ? 0 : 1) }
  ' "$workflow"
}

# Shared awk prelude: strip a trailing shell comment while honouring quotes.
STRIP_SHELL_COMMENT_AWK='
    function strip_shell_comment(text, i, ch, quote, escaped, previous) {
      quote = ""
      escaped = 0
      for (i = 1; i <= length(text); i++) {
        ch = substr(text, i, 1)
        if (escaped) {
          escaped = 0
          continue
        }
        if (quote == "\047") {
          if (ch == "\047") quote = ""
          continue
        }
        if (quote == "\"") {
          if (ch == "\\") {
            escaped = 1
          } else if (ch == "\"") {
            quote = ""
          }
          continue
        }
        if (ch == "\\") {
          escaped = 1
          continue
        }
        if (ch == "\047" || ch == "\"") {
          quote = ch
          continue
        }
        previous = i == 1 ? "" : substr(text, i - 1, 1)
        if (ch == "#" && (i == 1 || previous ~ /[[:space:];|&()]/)) {
          return substr(text, 1, i - 1)
        }
      }
      return text
    }
'

workflow_job_has_deterministic_npm() {
  local workflow="$1"
  local job="$2"
  local bootstrap="$3"
  local expected_ci="$4"

  awk -v job="$job" -v bootstrap="$bootstrap" -v expected_ci="$expected_ci" "$STRIP_SHELL_COMMENT_AWK"'

    function has_npm_token(text) {
      return text ~ /(^|[^[:alnum:]_])npm([^[:alnum:]_]|$)/
    }

    function has_npm_install(text) {
      return text ~ /(^|[^[:alnum:]_])npm([^[:alnum:]_].*)?[[:space:]]+(ci|install)([^[:alnum:]_]|$)/
    }

    $0 == "  " job ":" { in_job = 1; next }
    in_job && $0 ~ /^  [[:alnum:]_-]+:$/ { exit }
    !in_job { next }
    $0 == "        run: npm ci --ignore-scripts" {
      ci_count++
      next
    }
    bootstrap != "" && $0 == "        run: " bootstrap {
      bootstrap_count++
      next
    }
    {
      line = strip_shell_comment($0)
    }
    !continued_npm && line ~ /^[[:space:]]*(-[[:space:]]+)?name:/ { next }
    !continued_npm && line ~ /^[[:space:]]*$/ { next }
    continued_npm {
      continued_command = continued_command " " line
      if (has_npm_install(continued_command)) invalid = 1
      if (line !~ /\\[[:space:]]*$/) {
        continued_npm = 0
        continued_command = ""
      }
      next
    }
    has_npm_install(line) {
      invalid = 1
    }
    has_npm_token(line) && line ~ /\\[[:space:]]*$/ {
      continued_npm = 1
      continued_command = line
    }
    END {
      expected_bootstrap = bootstrap != "" ? 1 : 0
      exit(!invalid && ci_count == expected_ci && bootstrap_count == expected_bootstrap ? 0 : 1)
    }
  ' "$workflow"
}

release_workflow_has_deterministic_npm() {
  local workflow="$1"
  workflow_job_has_deterministic_npm "$workflow" prepublish "npm install -g --ignore-scripts npm@11.12.1" 1 || return 1
  workflow_job_has_deterministic_npm "$workflow" build "" 1 || return 1
  workflow_job_has_deterministic_npm "$workflow" prepare-npm "npm install -g --ignore-scripts npm@11.12.1" 1 || return 1
  workflow_job_has_deterministic_npm "$workflow" publish "npm install --global --ignore-scripts npm@11.19.0" 0 || return 1
  ! grep -Eq 'npm@latest' "$workflow"
}

release_workflow_is_provenance_only() {
  local workflow="$1"
  awk "$STRIP_SHELL_COMMENT_AWK"'

    $0 == "  publish:" { in_job = 1; next }
    in_job && $0 ~ /^  [[:alnum:]_-]+:$/ { exit }
    in_job && $0 !~ /^[[:space:]]*#/ {
      line = strip_shell_comment($0)
      while (match(line, /npm[[:space:]]+publish/)) {
        found = 1
        publish_start = RSTART
        publish_length = RLENGTH
        command = substr(line, publish_start)
        if (match(command, /(\|\||&&|;)/)) {
          command = substr(command, 1, RSTART - 1)
        }
        if (command !~ /(^|[[:space:]])--provenance([[:space:]]|$)/) {
          invalid = 1
        }
        line = substr(line, publish_start + publish_length)
      }
    }
    END { exit(found && !invalid ? 0 : 1) }
  ' "$workflow"
}

release_workflow_uses_strict_pack_check() {
  local workflow="$1"

  awk \
    -v target_name="      - name: Verify npm pack surfaces" \
    -v target_run="        run: node scripts/npm-pack-surface-check.mjs --require-release-artifacts" '
    function finish_step() {
      if (in_target && has_run) {
        found = 1
      }
    }

    $0 == "  prepare-npm:" {
      in_job = 1
      next
    }
    in_job && $0 ~ /^  [[:alnum:]_-]+:$/ {
      finish_step()
      exit
    }
    in_job && $0 ~ /^      - / {
      finish_step()
      in_target = ($0 == target_name)
      has_run = 0
      next
    }
    in_target && $0 == target_run { has_run = 1 }

    END {
      finish_step()
      exit(found ? 0 : 1)
    }
  ' "$workflow"
}

write_fake_surface_fixture() {
  local napi_dir="$1"
  local bin_dir="$2"
  local file

  mkdir -p \
    "$napi_dir/scripts" \
    "$napi_dir/node_modules/@napi-rs/cli/dist" \
    "$napi_dir/npm/wasm32-wasi" \
    "$napi_dir/npm/wasm32-wasi-singlethreaded" \
    "$bin_dir"
  cp "$ROOT/crates/oxc_coverage_instrument_napi/package.json" "$napi_dir/package.json"
  cp "$ROOT/crates/oxc_coverage_instrument_napi/npm/wasm32-wasi/package.json" \
    "$napi_dir/npm/wasm32-wasi/package.json"
  cp "$ROOT/crates/oxc_coverage_instrument_napi/npm/wasm32-wasi-singlethreaded/package.json" \
    "$napi_dir/npm/wasm32-wasi-singlethreaded/package.json"

  while IFS= read -r file; do
    printf 'original-root:%s\n' "$file" >"$napi_dir/$file"
  done < <(node -e 'for (const file of require(process.argv[1]).files) console.log(file)' "$napi_dir/package.json")
  printf 'original-root:%s\n' 'coverage-instrument.wasm32-wasi.wasm' \
    >"$napi_dir/coverage-instrument.wasm32-wasi.wasm"
  while IFS= read -r file; do
    printf 'original-threaded:%s\n' "$file" >"$napi_dir/npm/wasm32-wasi/$file"
  done < <(node -e 'for (const file of require(process.argv[1]).files) console.log(file)' \
    "$napi_dir/npm/wasm32-wasi/package.json")
  while IFS= read -r file; do
    printf 'original-single:%s\n' "$file" >"$napi_dir/npm/wasm32-wasi-singlethreaded/$file"
  done < <(node -e 'for (const file of require(process.argv[1]).files) console.log(file)' \
    "$napi_dir/npm/wasm32-wasi-singlethreaded/package.json")

  printf '%s\n' \
    "import { writeFileSync } from 'node:fs';" \
    "writeFileSync(new URL('../browser.js', import.meta.url), 'selector\\n');" \
    >"$napi_dir/scripts/patch-browser-loader.mjs"
  : >"$napi_dir/scripts/patch-wasi-browser-shim.mjs"
  printf '%s\n' \
    "import { copyFileSync, readFileSync } from 'node:fs';" \
    "const root = new URL('../', import.meta.url);" \
    "const target = new URL('../npm/wasm32-wasi-singlethreaded/', import.meta.url);" \
    "const manifest = JSON.parse(readFileSync(new URL('package.json', target), 'utf8'));" \
    "for (const file of manifest.files) copyFileSync(new URL(file, root), new URL(file, target));" \
    "if (process.env.FAIL_PREPARE_PACKAGE_SURFACE === '1') process.exit(9);" \
    >"$napi_dir/scripts/prepare-wasi-singlethreaded-package.mjs"
  printf '%s\n' \
    "import { readFileSync } from 'node:fs';" \
    "const target = new URL('../npm/wasm32-wasi-singlethreaded/', import.meta.url);" \
    "const manifest = JSON.parse(readFileSync(new URL('package.json', target), 'utf8'));" \
    "for (const file of manifest.files) {" \
    "  if (!readFileSync(new URL(file, target), 'utf8').startsWith('single:')) process.exit(8);" \
    "}" \
    >"$napi_dir/scripts/validate-wasi-singlethreaded-package.mjs"

  ln -s "$(command -v node)" "$bin_dir/node"
  printf '%s\n' '#!/bin/sh' 'exit 0' >"$bin_dir/npm"
  printf '%s\n' '#!/bin/sh' 'exit 0' >"$bin_dir/cargo"
  # Keep fake-tool variables literal so they expand only when the fake runs.
  # shellcheck disable=SC2016
  printf '%s\n' '#!/bin/sh' \
    'if [ "$1" = "target" ]; then printf "%s\n" wasm32-wasip1-threads wasm32-wasip1; fi' \
    >"$bin_dir/rustup"
  # shellcheck disable=SC2016
  printf '%s\n' '#!/bin/sh' \
    'kind=threaded' \
    'for arg in "$@"; do if [ "$arg" = "wasm32-wasip1" ]; then kind=single; fi; done' \
    'for file in $(node -e '\''for (const file of require(process.argv[1]).files) console.log(file)'\'' "$FAKE_NAPI_DIR/npm/wasm32-wasi/package.json"); do' \
    '  printf "%s:%s\n" "$kind" "$file" > "$FAKE_NAPI_DIR/$file"' \
    'done' \
    'printf "%s:index.js\n" "$kind" > "$FAKE_NAPI_DIR/index.js"' \
    'printf "%s:index.d.ts\n" "$kind" > "$FAKE_NAPI_DIR/index.d.ts"' \
    'printf "%s:browser.js\n" "$kind" > "$FAKE_NAPI_DIR/browser.js"' \
    >"$bin_dir/npx"
  # shellcheck disable=SC2016
  printf '%s\n' '#!/bin/sh' \
    'if [ "${FAIL_RESTORE_TARGET:-}" != "" ] && [ "${RESTORE_FAILURE_MARKER:-}" != "" ] && [ "$#" -eq 2 ] && [ "$1" = "-f" ] && [ "$2" = "$FAIL_RESTORE_TARGET" ] && [ ! -e "$RESTORE_FAILURE_MARKER" ]; then' \
    '  : >"$RESTORE_FAILURE_MARKER"' \
    '  echo "injected restore rm failure: $2" >&2' \
    '  exit 23' \
    'fi' \
    'exec "${REAL_RM:-/bin/rm}" "$@"' \
    >"$bin_dir/rm"
  chmod +x "$bin_dir/npm" "$bin_dir/cargo" "$bin_dir/rustup" "$bin_dir/npx" "$bin_dir/rm"
}

assert_prepared_surface() {
  local napi_dir="$1"
  local file

  assert_file_equal "$napi_dir/index.js" "threaded:index.js" "threaded root index.js"
  assert_file_equal "$napi_dir/index.d.ts" "threaded:index.d.ts" "threaded root index.d.ts"
  assert_file_equal "$napi_dir/browser.js" "selector" "root browser selector"
  while IFS= read -r file; do
    assert_file_equal "$napi_dir/npm/wasm32-wasi/$file" "threaded:$file" "threaded package $file"
  done < <(node -e 'for (const file of require(process.argv[1]).files) console.log(file)' \
    "$napi_dir/npm/wasm32-wasi/package.json")
  while IFS= read -r file; do
    assert_file_equal "$napi_dir/npm/wasm32-wasi-singlethreaded/$file" "single:$file" \
      "single-threaded package $file"
  done < <(node -e 'for (const file of require(process.argv[1]).files) console.log(file)' \
    "$napi_dir/npm/wasm32-wasi-singlethreaded/package.json")
}

if "$CHECK" unknown-profile >"$TMP/unknown.log" 2>&1; then
  fail "unknown profile succeeded"
fi
assert_contains "$TMP/unknown.log" "Unknown profile: unknown-profile"
assert_contains "$TMP/unknown.log" "Usage:"

mkdir -p "$TMP/empty-bin"
if PATH="$TMP/empty-bin" /bin/bash "$CHECK" typos >"$TMP/missing.log" 2>&1; then
  fail "direct profile succeeded without its prerequisite"
fi
assert_contains "$TMP/missing.log" "Required tool 'typos' is not installed."

mkdir -p "$TMP/old-node-bin"
printf '%s\n' '#!/bin/sh' 'echo 20' >"$TMP/old-node-bin/node"
chmod +x "$TMP/old-node-bin/node"
if PATH="$TMP/old-node-bin:/usr/bin:/bin" "$CHECK" inspector-smoke >"$TMP/old-node.log" 2>&1; then
  fail "inspector profile accepted an unsupported Node.js major"
fi
assert_contains "$TMP/old-node.log" "Node.js 22 is required for inspector-smoke."

mkdir -p "$TMP/old-python-bin"
printf '%s\n' '#!/bin/sh' 'echo 3.10' 'exit 1' >"$TMP/old-python-bin/python3"
chmod +x "$TMP/old-python-bin/python3"
if PATH="$TMP/old-python-bin:/usr/bin:/bin" "$CHECK" version-sync >"$TMP/old-python.log" 2>&1; then
  fail "version-sync accepted unsupported Python"
fi
assert_contains "$TMP/old-python.log" "Python 3.11 or newer with tomllib is required for version-sync. Found 3.10."

mkdir -p "$TMP/fake-bin"
# Keep fake-tool variables literal so they expand only when the fake runs.
# shellcheck disable=SC2016
printf '%s\n' '#!/bin/sh' 'printf "%s|%s\n" "$PWD" "$*" > "$FAKE_CARGO_LOG"' >"$TMP/fake-bin/cargo"
chmod +x "$TMP/fake-bin/cargo"
(
  cd "$TMP"
  FAKE_CARGO_LOG="$TMP/fmt-cargo.log" PATH="$TMP/fake-bin:/usr/bin:/bin" "$CHECK" fmt >"$TMP/fmt.log" 2>&1
)
assert_equal "$(<"$TMP/fmt-cargo.log")" "$ROOT|fmt --all --check" "external-cwd fmt invocation"

FAKE_CARGO_LOG="$TMP/pre-push-cargo.log" PATH="$TMP/fake-bin:/usr/bin:/bin" "$CHECK" pre-push >"$TMP/pre-push.log" 2>&1
assert_contains "$TMP/pre-push.log" "typos is not installed; skipping optional pre-push check"

mkdir -p "$TMP/ast-bin"
# shellcheck disable=SC2016
printf '%s\n' '#!/bin/sh' \
  'printf "%s|%s|%s\n" "$PWD" "${RUSTDOCFLAGS:-}" "$*" >>"$FAKE_AST_CARGO_LOG"' \
  >"$TMP/ast-bin/cargo"
chmod +x "$TMP/ast-bin/cargo"
FAKE_AST_CARGO_LOG="$TMP/ast-cargo.log" PATH="$TMP/ast-bin:/usr/bin:/bin" \
  "$CHECK" ast-api >"$TMP/ast-api.log" 2>&1
assert_equal "$(<"$TMP/ast-cargo.log")" "$ROOT||test -p oxc_coverage_instrument --features ast-api --all-targets
$ROOT||test -p oxc_coverage_instrument --features ast-api --doc
$ROOT||clippy -p oxc_coverage_instrument --features ast-api --all-targets -- -D warnings
$ROOT|-D warnings|doc -p oxc_coverage_instrument --features ast-api --no-deps --document-private-items" \
  "ast-api cargo commands"

bash "$ROOT/scripts/benchmark-comparison.sh" --self-test
node --test "$ROOT/scripts/tests/real-world-corpus.test.mjs"

fake_napi="$TMP/fake-napi"
fake_surface_bin="$TMP/fake-surface-bin"
fake_prepare="$TMP/prepare-package-surface.sh"
write_fake_surface_fixture "$fake_napi" "$fake_surface_bin"
sed "s#^NAPI_DIR=.*#NAPI_DIR=\"$fake_napi\"#" "$ROOT/scripts/prepare-package-surface.sh" \
  >"$fake_prepare"
chmod +x "$fake_prepare"
cp -R "$fake_napi" "$TMP/fake-napi-prerequisite"
rm -rf "$TMP/fake-napi-prerequisite/node_modules"
sed "s#^NAPI_DIR=.*#NAPI_DIR=\"$TMP/fake-napi-prerequisite\"#" \
  "$ROOT/scripts/prepare-package-surface.sh" >"$TMP/prepare-package-prerequisite.sh"
chmod +x "$TMP/prepare-package-prerequisite.sh"
cp -R "$TMP/fake-napi-prerequisite" "$TMP/fake-napi-prerequisite-before"
if FAKE_NAPI_DIR="$TMP/fake-napi-prerequisite" PATH="$fake_surface_bin:/usr/bin:/bin" \
  "$TMP/prepare-package-prerequisite.sh" >"$TMP/prepare-prerequisite.log" 2>&1; then
  fail "package preparation succeeded without N-API dependencies"
fi
if ! diff -qr "$TMP/fake-napi-prerequisite-before" "$TMP/fake-napi-prerequisite" \
  >"$TMP/prepare-prerequisite.diff"; then
  sed -n '1,120p' "$TMP/prepare-prerequisite.diff" >&2
  fail "package preparation changed destinations before prerequisites passed"
fi
cp -R "$fake_napi" "$TMP/fake-napi-before"
if FAKE_NAPI_DIR="$fake_napi" FAIL_PREPARE_PACKAGE_SURFACE=1 \
  PATH="$fake_surface_bin:/usr/bin:/bin" "$fake_prepare" >"$TMP/prepare-failure.log" 2>&1; then
  fail "package preparation failure injection succeeded"
fi
if ! diff -qr "$TMP/fake-napi-before" "$fake_napi" >"$TMP/prepare-restore.diff"; then
  sed -n '1,120p' "$TMP/prepare-restore.diff" >&2
  fail "package preparation did not restore destinations after failure"
fi
restore_failure_napi="$TMP/fake-napi-restore-failure"
restore_failure_prepare="$TMP/prepare-package-restore-failure.sh"
restore_failure_marker="$TMP/restore-failure.marker"
cp -R "$fake_napi" "$restore_failure_napi"
sed "s#^NAPI_DIR=.*#NAPI_DIR=\"$restore_failure_napi\"#" \
  "$ROOT/scripts/prepare-package-surface.sh" >"$restore_failure_prepare"
chmod +x "$restore_failure_prepare"
cp -R "$restore_failure_napi" "$TMP/fake-napi-restore-failure-before"
if FAKE_NAPI_DIR="$restore_failure_napi" FAIL_PREPARE_PACKAGE_SURFACE=1 \
  FAIL_RESTORE_TARGET="$restore_failure_napi/index.js" \
  RESTORE_FAILURE_MARKER="$restore_failure_marker" PATH="$fake_surface_bin:/usr/bin:/bin" \
  "$restore_failure_prepare" >"$TMP/prepare-restore-failure.log" 2>&1; then
  fail "package preparation succeeded after a restore operation failed"
fi
if [ ! -f "$restore_failure_marker" ]; then
  fail "restore operation failure injection did not run"
fi
assert_contains "$TMP/prepare-restore-failure.log" "injected restore rm failure"
assert_contains \
  "$TMP/prepare-restore-failure.log" \
  "prepare-package-surface: failed to restore original artifacts"
if ! diff -qr "$TMP/fake-napi-restore-failure-before" "$restore_failure_napi" \
  >"$TMP/prepare-restore-failure.diff"; then
  sed -n '1,120p' "$TMP/prepare-restore-failure.diff" >&2
  fail "package preparation stopped restoring after an operation failed"
fi
assert_contains "$TMP/prepare-restore-failure.log" "recovery snapshot retained at "
recovery_snapshot="$(sed -n 's/^prepare-package-surface: recovery snapshot retained at //p' \
  "$TMP/prepare-restore-failure.log")"
if [ ! -d "$recovery_snapshot" ]; then
  fail "package preparation removed the recovery snapshot after restore failure"
fi
rm -rf "$recovery_snapshot"
FAKE_NAPI_DIR="$fake_napi" PATH="$fake_surface_bin:/usr/bin:/bin" \
  "$fake_prepare" >"$TMP/prepare-success.log" 2>&1
assert_prepared_surface "$fake_napi"

node "$ROOT/scripts/npm-pack-surface-check.test.mjs"

(
  cd "$TMP"
  "$CHECK" --list >"$TMP/list.log"
)

expected_profiles=(
  fmt
  clippy
  rust-check
  rust-test-fast
  rust-test
  doc-test
  rust-doc
  ast-api
  typos
  version-sync
  napi-test
  wasi-shim-test
  browser-loader-test
  istanbul-diff-strict
  istanbul-diff-default
  istanbul-diff
  prepare-package-surface
  package-surface
  audit
  shear
  inspector-smoke
  real-world-output
  real-world-parity
  compose-real-map-parity
  real-world-gates
  istanbul-upstream
  vitest-typecheck
  vitest-coverage
  vitest-verify
  actionlint
  zizmor
  commitlint
  self-test
  rust
  pre-push
  all-local
)
expected_list="$(printf '%s\n' "${expected_profiles[@]}")"
actual_list="$(awk '{print $1}' "$TMP/list.log")"
assert_equal "$actual_list" "$expected_list" "profile inventory"

sed -n '/^run_all_local() {$/,/^}$/p' "$CHECK" >"$TMP/all-local-body.log"
assert_contains "$TMP/all-local-body.log" "run_self_test"
assert_contains "$TMP/all-local-body.log" "run_ast_api"
assert_contains "$TMP/all-local-body.log" "run_real_world_gates"

sed -n '/^run_version_sync() {$/,/^}$/p' "$CHECK" >"$TMP/version-sync-body.log"
assert_contains \
  "$TMP/version-sync-body.log" \
  "node scripts/npm-pack-surface-check.mjs --metadata-only"

workflow="$ROOT/.github/workflows/ci.yml"
if ! workflow_check_has_linux_profile_step "$workflow" "Verification runner self-test" self-test; then
  fail "CI check job is missing the Linux-only verification runner self-test step"
fi
if ! workflow_check_has_linux_profile_step "$workflow" "Check experimental AST API" "ast-api"; then
  fail "CI check job is missing the Linux-only ast-api profile"
fi
if ! workflow_real_world_job_is_gated "$workflow"; then
  fail "CI real-world-parity job is missing manifest-keyed preparation or repository gates"
fi
if ! workflow_job_has_need "$workflow" ci-ok real-world-parity; then
  fail "ci-ok.needs is missing real-world-parity"
fi
if ! workflow_job_has_need "$workflow" ci-ok version-sync; then
  fail "ci-ok.needs is missing version-sync"
fi
if ! workflow_version_sync_has_node_and_profile "$workflow"; then
  fail "CI version-sync job is missing Node.js 22 or the metadata validation profile"
fi
release_workflow="$ROOT/.github/workflows/release-npm.yml"
while read -r job dependency; do
  if ! workflow_job_has_need "$release_workflow" "$job" "$dependency"; then
    fail "release $job job must depend on $dependency"
  fi
  missing_release_need="$TMP/missing-$job-need.yml"
  awk -v job="$job" -v dependency="$dependency" '
    $0 == "  " job ":" { in_job = 1 }
    in_job && $0 ~ /^  [[:alnum:]_-]+:$/ && $0 != "  " job ":" { in_job = 0 }
    in_job && $0 == "    needs: " dependency { next }
    { print }
  ' "$release_workflow" >"$missing_release_need"
  assert_file_differs "$missing_release_need" "$release_workflow" "$job dependency"
  if workflow_job_has_need "$missing_release_need" "$job" "$dependency"; then
    fail "release dependency validator accepted missing $job -> $dependency"
  fi
done <<'RELEASE_DEPENDENCIES'
prepublish release-source
build prepublish
publish-crate build
prepare-npm publish-crate
publish prepare-npm
RELEASE_DEPENDENCIES
if ! release_workflow_has_deterministic_npm "$release_workflow"; then
  fail "npm release must use exact npm and strict lockfile installation"
fi
if ! release_workflow_is_provenance_only "$release_workflow"; then
  fail "every npm publish command must require provenance"
fi
if ! release_workflow_uses_strict_pack_check "$release_workflow"; then
  fail "npm release is missing strict platform artifact validation"
fi

invalid_self_test_workflow="$TMP/invalid-self-test-workflow.yml"
printf '%s\n' \
  'jobs:' \
  '  check:' \
  '    steps:' \
  '      - name: Verification runner self-test' \
  '        run: ./scripts/check.sh self-test' \
  '      - name: Unrelated Linux-only step' \
  "        if: runner.os == 'Linux'" \
  '        run: true' \
  '  msrv:' \
  '    runs-on: ubuntu-26.04' \
  >"$invalid_self_test_workflow"
if workflow_check_has_linux_profile_step "$invalid_self_test_workflow" "Verification runner self-test" self-test; then
  fail "CI self-test validator accepted fields split across steps"
fi

commented_need_workflow="$TMP/commented-need-workflow.yml"
printf '%s\n' \
  'jobs:' \
  '  ci-ok:' \
  '    needs:' \
  '      # - version-sync' \
  '    runs-on: ubuntu-26.04' \
  >"$commented_need_workflow"
if workflow_job_has_need "$commented_need_workflow" ci-ok version-sync; then
  fail "ci-ok.needs validator accepted a commented version-sync entry"
fi

unrelated_need_workflow="$TMP/unrelated-need-workflow.yml"
printf '%s\n' \
  'jobs:' \
  '  ci-ok:' \
  '    needs:' \
  '      - check' \
  '    metadata:' \
  '      - version-sync' \
  '    runs-on: ubuntu-26.04' \
  >"$unrelated_need_workflow"
if workflow_job_has_need "$unrelated_need_workflow" ci-ok version-sync; then
  fail "ci-ok.needs validator accepted version-sync from an unrelated property"
fi

invalid_version_sync_workflow="$TMP/invalid-version-sync-workflow.yml"
printf '%s\n' \
  'jobs:' \
  '  version-sync:' \
  '    steps:' \
  '      - uses: actions/setup-node@example' \
  '      - name: Unrelated step' \
  '        with:' \
  '          node-version: 22' \
  '      - run: ./scripts/check.sh version-sync' \
  '  audit:' \
  '    runs-on: ubuntu-26.04' \
  >"$invalid_version_sync_workflow"
if workflow_version_sync_has_node_and_profile "$invalid_version_sync_workflow"; then
  fail "CI version-sync validator accepted a Node.js version from another step"
fi

invalid_release_workflow="$TMP/invalid-release-workflow.yml"
printf '%s\n' \
  'jobs:' \
  '  prepare-npm:' \
  '    steps:' \
  '      - name: Verify npm pack surfaces' \
  '        run: node scripts/npm-pack-surface-check.mjs' \
  '      - name: Unrelated strict command' \
  '        run: node scripts/npm-pack-surface-check.mjs --require-release-artifacts' \
  '  release:' \
  '    runs-on: ubuntu-26.04' \
  >"$invalid_release_workflow"
if release_workflow_uses_strict_pack_check "$invalid_release_workflow"; then
  fail "npm release validator accepted strict mode from another step"
fi

invalid_crate_order="$TMP/invalid-crate-order.yml"
printf '%s\n' \
  'jobs:' \
  '  publish-crate:' \
  '    runs-on: ubuntu-26.04' \
  '  publish:' \
  '    needs: build' \
  >"$invalid_crate_order"
if workflow_job_has_need "$invalid_crate_order" publish-crate build; then
  fail "release ordering validator accepted build dependency from publish"
fi

invalid_npm_order="$TMP/invalid-npm-order.yml"
printf '%s\n' \
  'jobs:' \
  '  publish-crate:' \
  '    metadata:' \
  '      needs: publish-crate' \
  '  publish:' \
  '    runs-on: ubuntu-26.04' \
  >"$invalid_npm_order"
if workflow_job_has_need "$invalid_npm_order" publish publish-crate; then
  fail "release ordering validator accepted dependency from another job"
fi

mutable_npm_workflow="$TMP/mutable-npm-workflow.yml"
sed 's/npm@11\.12\.1/npm@latest/' "$release_workflow" >"$mutable_npm_workflow"
assert_file_differs "$mutable_npm_workflow" "$release_workflow" "mutable_npm_workflow"
if release_workflow_has_deterministic_npm "$mutable_npm_workflow"; then
  fail "release validator accepted npm@latest"
fi

mutable_publisher_workflow="$TMP/mutable-publisher-workflow.yml"
sed 's/npm@11\.19\.0/npm@latest/' "$release_workflow" >"$mutable_publisher_workflow"
assert_file_differs "$mutable_publisher_workflow" "$release_workflow" "mutable publisher"
if release_workflow_has_deterministic_npm "$mutable_publisher_workflow"; then
  fail "release validator accepted a mutable publisher tool"
fi

lifecycle_install_workflow="$TMP/lifecycle-install-workflow.yml"
sed 's/npm ci --ignore-scripts/npm ci/' "$release_workflow" >"$lifecycle_install_workflow"
assert_file_differs "$lifecycle_install_workflow" "$release_workflow" "lifecycle installation"
if release_workflow_has_deterministic_npm "$lifecycle_install_workflow"; then
  fail "release validator accepted dependency lifecycle scripts"
fi

publisher_dependencies_workflow="$TMP/publisher-dependencies-workflow.yml"
awk '
  { print }
  $0 == "        run: npm install --global --ignore-scripts npm@11.19.0" {
    print "      - name: Dependency installation in privileged publisher"
    print "        run: npm ci --ignore-scripts"
  }
' "$release_workflow" >"$publisher_dependencies_workflow"
assert_file_differs "$publisher_dependencies_workflow" "$release_workflow" "publisher dependencies"
if release_workflow_has_deterministic_npm "$publisher_dependencies_workflow"; then
  fail "release validator accepted project dependency installation in the publisher"
fi

non_strict_pack_workflow="$TMP/non-strict-pack-workflow.yml"
sed 's/npm-pack-surface-check.mjs --require-release-artifacts/npm-pack-surface-check.mjs/' \
  "$release_workflow" >"$non_strict_pack_workflow"
assert_file_differs "$non_strict_pack_workflow" "$release_workflow" "strict pack validation"
if release_workflow_uses_strict_pack_check "$non_strict_pack_workflow"; then
  fail "release validator accepted npm preparation without strict artifact validation"
fi

fallback_install_workflow="$TMP/fallback-install-workflow.yml"
awk '
  !changed && $0 == "        run: npm ci --ignore-scripts" {
    sub(/npm ci --ignore-scripts/, "npm ci --ignore-scripts || npm install")
    changed = 1
  }
  { print }
' "$release_workflow" >"$fallback_install_workflow"
assert_file_differs "$fallback_install_workflow" "$release_workflow" "fallback_install_workflow"
if release_workflow_has_deterministic_npm "$fallback_install_workflow"; then
  fail "release validator accepted fallback dependency installation"
fi

multiline_install_workflow="$TMP/multiline-install-workflow.yml"
awk '
  { print }
  !inserted && $0 == "        run: npm ci --ignore-scripts" {
    print "      - name: Multiline dependency installation"
    print "        run: |"
    print "          npm install"
    inserted = 1
  }
' "$release_workflow" >"$multiline_install_workflow"
assert_file_differs "$multiline_install_workflow" "$release_workflow" "multiline_install_workflow"
if release_workflow_has_deterministic_npm "$multiline_install_workflow"; then
  fail "release validator accepted multiline dependency installation"
fi

quoted_install_workflow="$TMP/quoted-install-workflow.yml"
awk '
  { print }
  !inserted && $0 == "        run: npm ci --ignore-scripts" {
    print "      - name: Quoted dependency installation"
    print "        run: \"npm install\""
    inserted = 1
  }
' "$release_workflow" >"$quoted_install_workflow"
assert_file_differs "$quoted_install_workflow" "$release_workflow" "quoted_install_workflow"
if release_workflow_has_deterministic_npm "$quoted_install_workflow"; then
  fail "release validator accepted quoted dependency installation"
fi

command_install_workflow="$TMP/command-install-workflow.yml"
awk '
  { print }
  !inserted && $0 == "        run: npm ci --ignore-scripts" {
    print "      - name: Shell-wrapped dependency installation"
    print "        run: |"
    print "          command npm install"
    inserted = 1
  }
' "$release_workflow" >"$command_install_workflow"
assert_file_differs "$command_install_workflow" "$release_workflow" "command_install_workflow"
if release_workflow_has_deterministic_npm "$command_install_workflow"; then
  fail "release validator accepted shell-wrapped dependency installation"
fi

silent_install_workflow="$TMP/silent-install-workflow.yml"
awk '
  { print }
  !inserted && $0 == "        run: npm ci --ignore-scripts" {
    print "      - name: Npm global option installation"
    print "        run: npm --silent install"
    inserted = 1
  }
' "$release_workflow" >"$silent_install_workflow"
assert_file_differs "$silent_install_workflow" "$release_workflow" "silent_install_workflow"
if release_workflow_has_deterministic_npm "$silent_install_workflow"; then
  fail "release validator accepted npm global option installation"
fi

prefix_ci_workflow="$TMP/prefix-ci-workflow.yml"
awk '
  { print }
  !inserted && $0 == "        run: npm ci --ignore-scripts" {
    print "      - name: Npm prefix clean installation"
    print "        run: npm --prefix path ci"
    inserted = 1
  }
' "$release_workflow" >"$prefix_ci_workflow"
assert_file_differs "$prefix_ci_workflow" "$release_workflow" "prefix_ci_workflow"
if release_workflow_has_deterministic_npm "$prefix_ci_workflow"; then
  fail "release validator accepted npm prefix clean installation"
fi

continued_install_workflow="$TMP/continued-install-workflow.yml"
awk '
  { print }
  !inserted && $0 == "        run: npm ci --ignore-scripts" {
    print "      - name: Continued dependency installation"
    print "        run: |"
    print "          npm --silent \\"
    print "            install"
    inserted = 1
  }
' "$release_workflow" >"$continued_install_workflow"
assert_file_differs "$continued_install_workflow" "$release_workflow" "continued_install_workflow"
if release_workflow_has_deterministic_npm "$continued_install_workflow"; then
  fail "release validator accepted continued dependency installation"
fi

inline_comment_workflow="$TMP/inline-comment-workflow.yml"
awk '
  { print }
  !inserted && $0 == "        run: npm ci --ignore-scripts" {
    print "      - name: Inline npm comment"
    print "        run: echo validated # npm install"
    inserted = 1
  }
' "$release_workflow" >"$inline_comment_workflow"
assert_file_differs "$inline_comment_workflow" "$release_workflow" "inline_comment_workflow"
if ! release_workflow_has_deterministic_npm "$inline_comment_workflow"; then
  fail "release validator rejected inline npm comment"
fi

extra_build_install_workflow="$TMP/extra-build-install-workflow.yml"
awk '
  { print }
  !inserted && $0 == "        run: npm ci --ignore-scripts" {
    print "      - name: Unrelated build install"
    print "        run: npm install"
    inserted = 1
  }
' "$release_workflow" >"$extra_build_install_workflow"
assert_file_differs "$extra_build_install_workflow" "$release_workflow" "extra_build_install_workflow"
if release_workflow_has_deterministic_npm "$extra_build_install_workflow"; then
  fail "release validator accepted extra npm install in build"
fi

extra_publish_install_workflow="$TMP/extra-publish-install-workflow.yml"
awk '
  { print }
  !inserted && $0 == "        run: npm install --global --ignore-scripts npm@11.19.0" {
    print "      - name: Unrelated publish install"
    print "        run: npm install example-package"
    inserted = 1
  }
' "$release_workflow" >"$extra_publish_install_workflow"
assert_file_differs "$extra_publish_install_workflow" "$release_workflow" "extra_publish_install_workflow"
if release_workflow_has_deterministic_npm "$extra_publish_install_workflow"; then
  fail "release validator accepted extra npm install in publish"
fi

chained_install_workflow="$TMP/chained-install-workflow.yml"
awk '
  { print }
  !inserted && $0 == "        run: npm ci --ignore-scripts" {
    print "      - name: Reversed install fallback"
    print "        run: npm install || npm ci"
    inserted = 1
  }
' "$release_workflow" >"$chained_install_workflow"
assert_file_differs "$chained_install_workflow" "$release_workflow" "chained_install_workflow"
if release_workflow_has_deterministic_npm "$chained_install_workflow"; then
  fail "release validator accepted chained npm install fallback"
fi

fallback_publish_workflow="$TMP/fallback-publish-workflow.yml"
awk '
  { print }
  !inserted && /npm publish.*--provenance/ {
    print "              npm publish \"$dir\" --access public"
    inserted = 1
  }
' "$release_workflow" >"$fallback_publish_workflow"
assert_file_differs "$fallback_publish_workflow" "$release_workflow" "fallback_publish_workflow"
if release_workflow_is_provenance_only "$fallback_publish_workflow"; then
  fail "release validator accepted publish without provenance"
fi

chained_publish_workflow="$TMP/chained-publish-workflow.yml"
awk '
  !changed && $0 == "            npm publish \"$file\" --access public --provenance --ignore-scripts" {
    $0 = $0 " || npm publish --access public --ignore-scripts"
    changed = 1
  }
  { print }
' "$release_workflow" >"$chained_publish_workflow"
assert_file_differs "$chained_publish_workflow" "$release_workflow" "chained_publish_workflow"
if release_workflow_is_provenance_only "$chained_publish_workflow"; then
  fail "release validator accepted same-line publish without provenance"
fi

commented_provenance_workflow="$TMP/commented-provenance-workflow.yml"
awk '
  !changed && $0 == "            npm publish \"$file\" --access public --provenance --ignore-scripts" {
    $0 = "            npm publish \"$file\" --access public # --provenance"
    changed = 1
  }
  { print }
' "$release_workflow" >"$commented_provenance_workflow"
assert_file_differs "$commented_provenance_workflow" "$release_workflow" "commented_provenance_workflow"
if release_workflow_is_provenance_only "$commented_provenance_workflow"; then
  fail "release validator accepted provenance inside a shell comment"
fi

quoted_hash_workflow="$TMP/quoted-hash-workflow.yml"
awk '
  !changed && $0 == "            npm publish \"$file\" --access public --provenance --ignore-scripts" {
    $0 = "            npm publish \"$file\" --access public --tag \"#stable\" --provenance --ignore-scripts"
    changed = 1
  }
  { print }
' "$release_workflow" >"$quoted_hash_workflow"
assert_file_differs "$quoted_hash_workflow" "$release_workflow" "quoted_hash_workflow"
if ! release_workflow_is_provenance_only "$quoted_hash_workflow"; then
  fail "release validator treated a quoted hash as a shell comment"
fi

while read -r documented_profile; do
  if ! grep -Eq "^${documented_profile}[[:space:]]" "$TMP/list.log"; then
    fail "CONTRIBUTING.md references unknown check profile '$documented_profile'"
  fi
done < <(sed -n 's#.*\./scripts/check\.sh \([a-z][a-z0-9-]*\).*#\1#p' "$ROOT/CONTRIBUTING.md")

echo "check self-test: PASS"
