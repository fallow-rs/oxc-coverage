# Vitest on real projects

Runs the test suites of four open-source projects with Vitest's default
Istanbul instrumenter and with the oxc instrumenter, and compares the time and
the coverage numbers.

| Project | What it is | Vitest | Test setup |
|:--|:--|:--|:--|
| [es-toolkit](https://github.com/toss/es-toolkit) | Utility library | 4.1.10 | Already uses the Istanbul provider; Yarn Plug'n'Play |
| [date-fns](https://github.com/date-fns/date-fns) | Date library | 5.0.1 | Vitest projects; most tests in browser mode (Chromium) |
| [hono](https://github.com/honojs/hono) | Web framework | 5.0.1 | Vitest projects; JSX; the `main` project runs |
| [h3](https://github.com/unjs/h3) | HTTP framework | 5.0.5 | Plain Node tests |

`projects.json` pins each project to a commit.

## How to switch a real project

The change in each project is the one in the root
[examples README](../README.md): install the package and add the
`instrumenter` line to the existing coverage config. The script does this
without editing the project's config. It writes `vitest.oxc-bench.config.mts`,
which imports the project's own config and merges only the coverage settings
into it, so `include`, `exclude`, and the Vitest projects stay as they are.

## What the script measures

Each project runs in four modes:

| Mode | Coverage |
|:--|:--|
| `none` | No coverage. The baseline. |
| `istanbul` | The Istanbul provider with Vitest's default instrumenter. |
| `oxc` | The Istanbul provider with `createOxcInstrumenter({ compat: 'istanbul' })`. |
| `v8` | The V8 provider, for reference. |

- **Instrumentation time.** The script times the Istanbul provider's
  transform hook, `onFileTransform`, which runs the instrumenter for each
  file. This is the step that the oxc instrumenter replaces, measured the same
  way in both modes. The hook also does some source-map work of its own, so
  the speedup of the instrumenter alone is a little higher.
- **Wall time.** The time of the whole `vitest run`, with the lowest and
  highest run. It also contains the test execution, which varies by several
  seconds between runs, so read it as context.
- **Coverage totals.** The `istanbul` and `oxc` modes must give the same
  covered and total counts for lines, statements, functions, and branches.
  The script exits with code 1 when they differ, or when the oxc instrumenter
  was not called.
- **Cold runs.** Vitest 5 caches transformed modules on disk, including the
  instrumented code. The script deletes the caches before every run, as in a
  CI run. A warm local rerun does not instrument the files again.

Vitest's default instrumenter differs by version. Vitest 5 uses the
Babel-based `@vitest/istanbul-lib-instrument`; Vitest 4 bundles
`istanbul-lib-instrument`. Both instrument each file twice: once for the code,
once more for a source map without the coverage counters.

## Results

The CI workflow `Vitest real projects` runs this script on a GitHub-hosted
runner and writes the tables below to its step summary.

<!-- results -->

## Run it yourself

```bash
node examples/vitest-real-projects/run.mjs --runs=5
```

The script clones the projects into `.bench-tmp/vitest-projects`, installs them
with their own package manager through `corepack`, and adds
`@vitest/coverage-istanbul`, `@vitest/coverage-v8`, and
`oxc-coverage-instrument` at the versions that match. For date-fns it installs
the headless Chromium of the project's Playwright version. Options:

- `--runs=<n>`: cold runs per mode (default 3);
- `--only=hono,h3`: a subset of the projects;
- `--oxc=<version>`: the `oxc-coverage-instrument` version from npm;
- `--markdown=<file>`: also write the tables to a file.

Close other heavy programs first. A busy machine slows the default instrumenter
and the test runs more than the oxc instrumenter, which makes the speedup look
larger than it is.

## Notes per project

- **hono**: only the `main` project runs; the runtime projects need Deno, Bun,
  workerd, or Fastly. The two test files under `build/` fail in every mode,
  with and without coverage: they check the published declarations and need a
  `dist` build first.
- **h3**: `test/body-limit.test.ts` is excluded. One of its tests times out
  after 5 s in some runs, in every mode, which adds noise to the timings.
- **date-fns**: browser mode measures the instrumentation in the Vite server.
  Most of the extra time of Istanbul coverage in browser mode comes from
  collecting the counters in the browser, which the instrumenter does not
  change.
