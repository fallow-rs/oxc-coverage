# Vitest on real projects

Runs the test suites of four open-source projects with Vitest's default
Istanbul instrumenter and with the `oxc-coverage-instrument` instrumenter, and
compares the time and
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
without editing the project's config. It writes `vitest.coverage-bench.config.mts`,
which imports the project's own config and merges only the coverage settings
into it, so `include`, `exclude`, and the Vitest projects stay as they are.

## What the script measures

Each project runs in four modes:

| Mode | Coverage |
|:--|:--|
| `none` | No coverage. The baseline. |
| `istanbul` | The Istanbul provider with Vitest's default instrumenter. |
| `oxc-coverage-instrument` | The Istanbul provider with `createOxcInstrumenter({ compat: 'istanbul' })`. |
| `v8` | The V8 provider, for reference. |

- **Instrumentation time.** The script times the Istanbul provider's
  transform hook, `onFileTransform`, which runs the instrumenter for each
  file. This is the step that `oxc-coverage-instrument` replaces, measured the same
  way in both modes. The hook also does some source-map work of its own, so
  the speedup of the instrumenter alone is a little higher.
- **Wall time.** The time of the whole `vitest run`, with the lowest and
  highest run. It also contains the test execution, which varies by several
  seconds between runs, so read it as context.
- **Coverage totals.** The `istanbul` and `oxc-coverage-instrument` modes must give the same
  covered and total counts for lines, statements, functions, and branches.
  The script exits with code 1 when they differ, or when the `oxc-coverage-instrument` instrumenter
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

Measured in [CI run 40326434759](https://github.com/fallow-rs/oxc-coverage/actions/runs/40326434759) on a GitHub-hosted runner (Ubuntu 24.04, Intel Xeon Platinum 8370C, 4 cores), Node.js 24.21.0, `oxc-coverage-instrument@0.18.0`, 3 cold runs per mode.

### Instrumentation

Time in the Istanbul provider's transform hook, summed over all files (median):

| Project | Files instrumented | Vitest's instrumenter | `oxc-coverage-instrument` | Speedup |
|:--|--:|--:|--:|--:|
| es-toolkit | 802 | 5.5 s | 1.0 s | 5.6x |
| date-fns | 1061 | 3.9 s | 0.5 s | 7.2x |
| hono | 636 | 10.7 s | 0.9 s | 11.6x |
| h3 | 81 | 1.4 s | 0.1 s | 11.6x |

### Whole test run

Wall time of `vitest run` (median). The coverage overhead is the time above the
run without coverage:

| Project | Test files | No coverage | Istanbul, Vitest's instrumenter | Istanbul, `oxc-coverage-instrument` | V8 | Overhead, Vitest's instrumenter | Overhead, `oxc-coverage-instrument` |
|:--|--:|--:|--:|--:|--:|--:|--:|
| es-toolkit | 720 | 172.8 s | 185.7 s | 181.7 s | 187.6 s | 12.9 s | 8.9 s |
| date-fns | 267 | 6.9 s | 34.0 s | 31.9 s | 8.4 s | 27.1 s | 25.0 s |
| hono | 122 | 21.0 s | 27.3 s | 24.0 s | 24.3 s | 6.3 s | 3.0 s |
| h3 | 70 | 13.2 s | 18.5 s | 16.4 s | 15.7 s | 5.3 s | 3.2 s |

### Coverage

The two Istanbul runs give the same covered and total counts for lines,
statements, functions, and branches in all four projects. For example, hono:
6265/6483 lines, 6495/6720 statements, 1089/1135 functions, and 4112/4408
branches in both runs. The V8 provider counts a little differently, as usual.

### What this means

- **The instrumentation step is 5.6x to 11.6x faster.** That step is where the
  `oxc-coverage-instrument` works, and the coverage numbers stay the same.
- **In Node.js tests, the coverage overhead drops by 30% to 52%.** hono goes
  from 6.3 s to 3.0 s of overhead, h3 from 5.3 s to 3.2 s, and es-toolkit from
  12.9 s to 8.9 s. The rest of the overhead was not measured separately; it
  includes running the counters in the tests and writing the report.
- **Istanbul with `oxc-coverage-instrument` is about as fast as V8 in Node.js tests**: faster than V8
  for es-toolkit and hono, slightly slower for h3. Istanbul coverage then no
  longer costs extra time, and it keeps its exact branch and function counts.
- **In browser mode the instrumenter is a small part of the overhead.** For
  date-fns, Istanbul coverage adds 27.1 s, of which 3.9 s is instrumentation;
  `oxc-coverage-instrument` takes 3.4 s off that. Most of the rest goes to running the instrumented
  code and collecting the counters in the browser. V8 coverage is much
  faster there, but it works only in Chromium.
- **A warm rerun gains less.** Vitest 5 caches the instrumented modules on
  disk, so a local rerun skips the instrumentation step.

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
- `--package-version=<version>`: the `oxc-coverage-instrument` version from npm;
- `--markdown=<file>`: also write the tables to a file.

Close other heavy programs first. A busy machine slows the default instrumenter
and the test runs more than `oxc-coverage-instrument`, which makes the speedup look
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
