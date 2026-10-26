# Vitest CI reports example

Vitest writes the raw coverage, and `oxc-coverage-instrument` writes the
reports that a CI system reads, and fails the run under a coverage threshold.

## What this shows

- `vitest.config.ts` uses the `oxc-coverage-instrument` instrumenter and only the `json` reporter, so
  Vitest writes `coverage/coverage-final.json` and nothing else.
- `scripts/report.mjs` reads that file once and writes:
  - `coverage/lcov.info` for Codecov, Coveralls, and SonarQube;
  - `coverage/cobertura-coverage.xml` for GitLab, Azure DevOps, and Jenkins;
  - `coverage/html/` for people;
  - a text summary on stdout, and a table in the GitHub Actions step summary
    when `GITHUB_STEP_SUMMARY` is set.
- The script exits with code 1 when a metric is under its threshold
  (`--min-lines=80` by default; add `--min-branches=90` and so on).
- `rootDir` makes the paths in `lcov` and `cobertura` relative to the
  repository, which most CI services expect.

In this example the `lcov` and `cobertura` reporters are 2.6x to 2.9x faster
than `istanbul-reports`. The `html` report is slower, because it highlights the
source code when it writes the report. See the benchmarks in the root README.

## Run

```bash
cd examples/vitest-ci-reports
npm install
npm run coverage
npm run report
npm run verify
```

In GitHub Actions:

```yaml
- run: npx vitest run --coverage
- run: node scripts/report.mjs --min-lines=80
- uses: codecov/codecov-action@v5
  with:
    files: coverage/lcov.info
```
