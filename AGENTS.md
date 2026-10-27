# Repository guidelines

Unofficial Istanbul-compatible coverage tooling built on Oxc: instrumentation, the
Istanbul data model, source-map remapping, V8-to-Istanbul conversion, and report
emitters, as one Cargo workspace with a Node binding on top.

See [CONTRIBUTING.md](CONTRIBUTING.md) for setup, commit rules, version policy,
and how to submit a change.

## Workspace layout

| Crate | Published | Owns |
|:------|:----------|:-----|
| `crates/oxc_coverage_instrument` | crates.io | Parse, transform, codegen. The instrumenter itself. |
| `crates/oxc_coverage_types` | crates.io | `FileCoverage`, `FnEntry`, `BranchEntry`, `Location`. |
| `crates/oxc_coverage_source_maps` | crates.io | Remapping `FileCoverage` back through a source map. |
| `crates/oxc_coverage_v8` | crates.io | V8 inspector range coverage to Istanbul. |
| `crates/oxc_coverage_report` | crates.io | Summary metrics, report tree, visitor protocol. |
| `crates/oxc_coverage_reports` | crates.io | `text`, `text-summary`, `json-summary`, `lcov`, `cobertura`, `html`. |
| `crates/oxc_coverage_instrument_cli` | no | The `oxc-coverage-instrument` binary. |
| `crates/oxc_coverage_instrument_napi` | npm | N-API binding and the published npm package. |

Rust source and tests live with their owning crate: unit tests in `src/`,
integration tests in `tests/`, benchmarks in `benches/`. The conformance corpus
and its Istanbul reference output are in
`crates/oxc_coverage_instrument/tests/conformance/`.

`crates/oxc_coverage_instrument_napi/package.json` is the published npm
manifest. The root `package.json` is a private helper manifest for the scripts
in `scripts/`. Platform manifests under
`crates/oxc_coverage_instrument_napi/npm/` must stay consistent with the
published manifest.

## Generated artifacts

In `crates/oxc_coverage_instrument_napi/`, the files `index.js`, `index.d.ts`,
`browser.js`, the native binaries, the WebAssembly binaries, and the WASI
loaders and workers are build outputs. They are produced by `napi build` and the
patch scripts under `crates/oxc_coverage_instrument_napi/scripts/`. Change the
input or the patch script, regenerate, then confirm no unexpected drift remains.

## Commands

`./scripts/check.sh` is the entry point for every check that can run on a
developer machine. `./scripts/check.sh --list` prints all profiles. The ones
used most often:

```bash
./scripts/check.sh rust-check      # cargo check --workspace
./scripts/check.sh rust-test       # cargo test --workspace --all-targets
./scripts/check.sh doc-test        # cargo test --workspace --doc
./scripts/check.sh clippy          # all + pedantic + nursery, warnings denied
./scripts/check.sh fmt             # cargo fmt --all --check
./scripts/check.sh rust-doc        # cargo doc with RUSTDOCFLAGS=-D warnings
./scripts/check.sh ast-api         # feature tests, doctests, clippy, and rustdoc; separate from defaults
./scripts/check.sh typos           # requires typos-cli
./scripts/check.sh version-sync    # internal version pins and release topology
./scripts/check.sh real-world-gates # both pinned production-source parity checks
./scripts/check.sh pre-push        # what the pre-push hook runs
./scripts/check.sh all-local       # every host-reproducible profile
```

Each profile fails with the exact prerequisite command when a tool or artifact
is missing. It never installs a Rust target or an npm dependency on your behalf.

`all-local` needs Node.js 22, npm dependencies installed at the repository root,
in the N-API crate, and in the Vitest example, plus `typos`, `cargo-audit`,
`cargo-shear`, `actionlint`, and `uv`. Build the native N-API artifact and both
generated WASI package surfaces first:

```bash
npm install
npm --prefix crates/oxc_coverage_instrument_napi install
npm --prefix crates/oxc_coverage_instrument_napi run build:debug
npm --prefix examples/vitest-typescript install
node scripts/prepare-real-world-corpus.mjs
./scripts/check.sh prepare-package-surface
./scripts/check.sh all-local
```

`check.sh` verifies the real-world corpus but never downloads it. Corpus updates
are deliberate: review the upstream source and license, update the exact version,
URL, SHA-256 digest, filename, and SPDX identifier in
`scripts/real-world-corpus.json`, then regenerate dated comparison evidence.

## Node binding

```bash
cd crates/oxc_coverage_instrument_napi
npm install
npx napi build --platform
cd ../..
./scripts/check.sh napi-test
./scripts/check.sh wasi-shim-test
```

`crates/oxc_coverage_instrument_napi/test.mjs` uses `node:assert` and is run
directly by `node`, not through a test runner.

## Conformance

The conformance tests compare output against `istanbul-lib-instrument` on a
shared fixture corpus. To regenerate the reference data after adding a fixture:

```bash
npm install
node crates/oxc_coverage_instrument/tests/conformance/generate-reference.mjs
```

`./scripts/check.sh istanbul-diff` runs both profile gates over the same corpus:
the strict profile must match Istanbul byte for byte, and the default profile
may differ only through extensions documented in the README.

## Scripts

| Script | Purpose |
|:-------|:--------|
| `scripts/check.sh` | Dispatcher for every local verification profile. |
| `scripts/istanbul-diff.mjs` | Strict byte diff plus the default-profile documented-delta gate. |
| `scripts/istanbul-upstream-specs.mjs` | Runtime cases taken from upstream Istanbul specs. |
| `scripts/real-world-output.mjs` | Runtime behaviour and counter placement on production-like sources. |
| `scripts/real-world-parity.mjs` | Strict-profile entry parity and default-profile count parity over the benchmark corpus. |
| `scripts/prepare-real-world-corpus.mjs` | Explicit download and SHA-256 verification of the pinned real-project corpus. |
| `scripts/v8-inspector-smoke.mjs` | Conversion of real Node inspector coverage. |
| `scripts/v8-branch-parity.mjs` | V8 branch arm counts against istanbul-lib-instrument, case by case. |
| `scripts/native-vs-wasm-parity.mjs` | Native binding output against the WASM binding, fixture by fixture. |
| `scripts/compose-eager-smoke.mjs` | Eager compose parity with the deferred remap, plus the mapping-boundary sweep. |
| `scripts/compose-real-map-parity.mjs` | The same parity over real babel-emitted maps for the benchmark corpus. |
| `scripts/benchmark-comparison.sh` | Timing comparison against the Istanbul, Babel, and SWC instrumenters. |
| `scripts/replacement-benchmarks.mjs` | Timing of each coverage stage against the Istanbul package it replaces and its Vitest fork. |
| `scripts/compare-istanbul.mjs` | Reference-output dumper for investigating shape differences. |
| `scripts/check-version-sync.sh` | Internal version pins and release-workflow coverage. |
| `scripts/sync-npm-versions.sh` | Version sync across the npm and platform manifests. |
| `scripts/prepare-package-surface.sh` | Builds both generated WASI packages for `package-surface`. |

## What only runs in CI

CI owns the operating-system and target matrices, MSRV toolchain selection, the
clean-worktree postcondition, WASM builds and tests, native-versus-WASM parity,
the platform examples, coverage generation and upload, artifact handling, and
release publication. The Typos and Cargo Deny jobs are action-owned; their
setup, matrices, and artifact inputs are not reproduced by `all-local`, which
prints that remainder rather than claiming it passed.

`.github/workflows/release-npm.yml` is authoritative for release topology and
publication behaviour.

## Code conventions

- Rust 2024 edition, MSRV 1.96.
- Strict clippy: all, pedantic, nursery, plus the workspace restriction lints.
- `cargo fmt` with `style_edition = "2024"` and `use_small_heuristics = "Max"`.
- `#[expect(..., reason = "...")]` rather than `#[allow]`.
- Doc comments on public types and functions.
- A new statement, branch, or function construct needs a test that pins its
  shape against Istanbul.
- Signed commits with Conventional Commit subjects. Inspect the staged scope
  before committing and leave unrelated worktree changes alone.

A fix is finished when a minimal reproduction test passes, the fix has been run
against at least one real project, and the suites for the affected surfaces
pass. Never copy private project names, sources, output, or credentials into
fixtures, documentation, commit messages, or release notes.
