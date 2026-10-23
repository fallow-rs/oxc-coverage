# Contributing

Thanks for your interest in contributing.

## Getting started

```bash
git clone https://github.com/fallow-rs/oxc-coverage
cd oxc-coverage-instrument
git config core.hooksPath .githooks
cargo build
cargo test --workspace
cargo run -p oxc_coverage_instrument --example instrument
```

[AGENTS.md](AGENTS.md) describes the workspace layout, the check profiles, the
helper scripts, and which checks only run in CI.

## Git hooks

The hooks under `.githooks/` mirror the CI jobs that block a merge.

- `pre-push` runs `./scripts/check.sh pre-push`.
- `commit-msg` lints the message against the Conventional Commits rules. It
  skips itself when commitlint is not installed, so a Rust-only contributor is
  never blocked on a Node toolchain.

Enable them once per clone:

```bash
git config core.hooksPath .githooks
```

`typos` is optional but recommended, since the pre-push aggregate skips it when
absent and says so:

```bash
cargo install typos-cli
```

Two environment variables change what `pre-push` does:

```bash
RUN_TESTS=1 git push      # also run cargo test --workspace
SKIP_PRE_PUSH=1 git push  # skip the hook entirely
```

## Dependency and supply-chain gates

CI runs `cargo audit`, `cargo deny`, and `cargo shear` on every push. Reproduce
them before touching a `Cargo.toml`:

```bash
cargo install cargo-audit && ./scripts/check.sh audit
cargo install cargo-deny && cargo deny check
cargo install cargo-shear && ./scripts/check.sh shear
```

## Workflow files

CI runs `actionlint` and `zizmor` against anything under `.github/workflows/`
and `.github/actions/`:

```bash
brew install actionlint
brew install uv
./scripts/check.sh actionlint
./scripts/check.sh zizmor
```

Install `uv` rather than `zizmor` itself. CI pins an exact zizmor version in the
`zizmor` job of `ci.yml`, because a new release can add an audit that reddens a
workflow which passed review. `./scripts/check.sh zizmor` reads that pin back
out of the workflow and runs the identical build through `uvx`. With a
system-installed zizmor it falls back to whatever version happens to be present
and warns that a local pass no longer implies CI will pass. To move the pin,
edit the version in `ci.yml`; the local target follows.

Every external action is SHA-pinned with a version comment
(`uses: owner/action@<40-hex> # vX.Y.Z`). Floating `@v6` and `@main` tags are
rejected by zizmor's policy.

## Commit messages

CI enforces Conventional Commits via commitlint. Config lives in
`commitlint.config.mjs`. Allowed types: `build`, `chore`, `ci`, `docs`, `feat`,
`fix`, `perf`, `refactor`, `revert`, `style`, `test`.

```bash
npm ci --ignore-scripts
./scripts/check.sh commitlint
```

## Version policy

- `oxc_coverage_instrument` and the published npm package move together and
  share the public package version.
- The companion crates (`oxc_coverage_types`, `oxc_coverage_source_maps`,
  `oxc_coverage_v8`, `oxc_coverage_report`, `oxc_coverage_reports`) carry their
  own versions and may stay on lower 0.x numbers while their APIs settle.
- The unpublished crates (`oxc_coverage_instrument_cli`,
  `oxc_coverage_instrument_napi`) do not define the public release version.
- `./scripts/check.sh version-sync` checks that every internal path dependency
  pins the version its target crate actually declares, and that every
  publishable crate appears in the release workflow.

## Submitting changes

1. Fork the repository and branch from `main`.
2. Make the change, with tests.
3. Run `./scripts/check.sh all-local`.
4. Open a pull request describing what changed and why.

The pull request body must contain a `Closes #N`, `Fixes #N`, or `Resolves #N`
keyword for any issue it closes, or the literal `N/A` if it closes none. A
workflow rejects bodies that contain neither; the check is skipped for `[bot]`
authors.

## License

By contributing, you agree that your contributions will be licensed under the
MIT License.
