# oxc-coverage-instrument CLI

The `oxc-coverage-instrument` binary: instrument a file, or render a coverage
report.

> **Unofficial.** Independent community project built on Oxc crates. Not part
> of, affiliated with, or endorsed by the Oxc project.

## Overview

The CLI is a thin front end over the suite. It instruments a single file and
writes the result to stdout or to disk, and it renders a `coverage-final.json`
into any of the shipped report formats.

```bash
cargo install --git https://github.com/fallow-rs/oxc-coverage oxc_coverage_instrument_cli
```

## Key Features

Instrumentation, either bare or through the explicit `instrument` subcommand:

```bash
oxc-coverage-instrument src/app.js                          # instrumented code to stdout
oxc-coverage-instrument src/app.js -o dist/app.js           # and <output>.map.json beside it
oxc-coverage-instrument src/app.js --coverage-map           # coverage map only
oxc-coverage-instrument src/app.js -o dist/app.js --source-map
```

`--coverage-map` writes JSON to stdout and cannot be combined with `-o` or
`--source-map`.

Reporting, through the `report` subcommand:

```bash
oxc-coverage-instrument report --format text         coverage-final.json
oxc-coverage-instrument report --format text-summary coverage-final.json
oxc-coverage-instrument report --format json-summary coverage-final.json -o coverage-summary.json
oxc-coverage-instrument report --format lcov         --root . coverage-final.json -o lcov.info
oxc-coverage-instrument report --format cobertura    --root . coverage-final.json -o cobertura.xml
oxc-coverage-instrument report --format html         --root . coverage-final.json --output-dir coverage/

# Render, then exit 2 if aggregate line coverage falls below the floor
oxc-coverage-instrument report --format text-summary coverage-final.json --fail-under 80
```

Exit codes are part of the CLI contract:

| Code | Meaning |
| ---: | :--- |
| 0 | Success |
| 1 | Usage, parsing, I/O, or rendering failure |
| 2 | Aggregate line coverage is below `--fail-under` |
| 3 | The coverage map is valid but contains no files |

`lcov` and `cobertura` use `--root` to relativize source paths, defaulting to the
working directory. Repo-relative paths are required by self-hosted Codecov, the
GitLab merge-request widget, Jenkins, and Azure DevOps.

`html` writes a self-contained directory tree to `--output-dir`, defaulting to
`coverage/`. Detail pages show the original source with per-line hit, miss, and
partial-branch colouring, reading source from disk via `--root`. Files carrying
an `inputSourceMap` are remapped through `oxc_coverage_source_maps`, so
TypeScript and JSX projects show original source rather than instrumented
JavaScript.

Run `oxc-coverage-instrument --help` for the full flag list.

## Architecture

The binary owns argument parsing, file IO, and the process exit code, and nothing
else. Instrumentation is delegated to `oxc_coverage_instrument`, summarization to
`oxc_coverage_report`, and rendering to `oxc_coverage_reports` through its
`Format` enum, so a format added to the reporters crate reaches the CLI as a new
`--format` value rather than as new rendering code here.

This crate is workspace-local and not published to crates.io; install it from git
or build it from a checkout.
