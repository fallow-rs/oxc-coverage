# Vitest on real projects

Runs the test suites of four open-source projects with Vitest's default
Istanbul instrumenter and with the `oxc-coverage-instrument` instrumenter, and
compares the time and the coverage numbers.

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

Each project runs in five modes:

| Mode | Coverage |
|:--|:--|
| `none` | No coverage. The baseline. |
| `istanbul` | The Istanbul provider with Vitest's default instrumenter. |
| `oxc-coverage-instrument` | The Istanbul provider with `createOxcInstrumenter({ compat: 'istanbul' })`. |
| `oxc-coverage-instrument-default` | The same with the default profile, to show what it adds to the coverage data. |
| `v8` | The V8 provider, for reference. |

- **Instrumentation time.** The script times the Istanbul provider's
  transform hook, `onFileTransform`, which runs the instrumenter for each
  file. This is the step that `oxc-coverage-instrument` replaces, measured the same
  way in both modes. The hook also does some source-map work of its own, so
  the speedup of the instrumenter alone is a little higher.
- **Steps after the test run.** The script times the provider methods that
  run in the main Vitest process after the tests: `readCoverageFiles` (read
  and merge, with the remap inside it), `getCoverageMapForUncoveredFiles`
  (untested files), `generateCoverage` (all of these), and `generateReports`.
  It sets Vitest's default reporters in every mode, so the report step is the
  same as in a project without a `reporter` setting.
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

Measured in [CI run 40417479338](https://github.com/fallow-rs/oxc-coverage/actions/runs/40417479338) on a GitHub-hosted runner (Ubuntu 24.04, AMD EPYC 7763, 4 cores), Node.js 24.21.0, `oxc-coverage-instrument@0.18.0`, 3 cold runs per mode.

### Instrumentation

Time in the Istanbul provider's transform hook, summed over all files (median):

| Project | Files instrumented | Vitest's instrumenter | `oxc-coverage-instrument` | Speedup |
|:--|--:|--:|--:|--:|
| es-toolkit | 802 | 5.7 s | 1.0 s | 5.7x |
| date-fns | 1061 | 4.0 s | 0.5 s | 7.5x |
| hono | 636 | 11.8 s | 0.9 s | 13.3x |
| h3 | 81 | 1.5 s | 0.1 s | 13.4x |

The times are rounded; the speedup comes from the unrounded times.

### Steps after the test run

Time in the main Vitest process after the tests (median). The test files write
their coverage to disk; Vitest then reads and merges it, remaps it through the
source maps, adds the included files that no test loaded, and writes the
reports. For V8, the remap column also holds the conversion to Istanbul data.
The reports are Vitest's default reporters (`text`, `html`, `clover`, `json`)
plus `json-summary`:

| Project | Provider | Read and merge | Remap | Untested files | Reports | Total |
|:--|:--|--:|--:|--:|--:|--:|
| es-toolkit | Istanbul, Vitest's instrumenter | 0.96 s | 0.35 s | 0.49 s | 0.57 s | 2.37 s |
| es-toolkit | Istanbul, `oxc-coverage-instrument` | 1.02 s | 0.57 s | 0.44 s | 0.56 s | 2.58 s |
| es-toolkit | V8 | 2.58 s | 1.05 s | 0.24 s | 0.61 s | 4.46 s |
| date-fns | Istanbul, Vitest's instrumenter | 3.03 s | 0.13 s | 0.00 s | 0.36 s | 3.53 s |
| date-fns | Istanbul, `oxc-coverage-instrument` | 3.16 s | 0.14 s | 0.00 s | 0.36 s | 3.66 s |
| date-fns | V8 | 0.09 s | 0.46 s | 0.00 s | 0.37 s | 0.91 s |
| hono | Istanbul, Vitest's instrumenter | 2.32 s | 0.25 s | 0.00 s | 0.56 s | 3.14 s |
| hono | Istanbul, `oxc-coverage-instrument` | 2.13 s | 0.32 s | 0.00 s | 0.57 s | 3.02 s |
| hono | V8 | 0.52 s | 1.09 s | 0.00 s | 0.63 s | 2.24 s |
| h3 | Istanbul, Vitest's instrumenter | 0.76 s | 0.04 s | 0.03 s | 0.08 s | 0.91 s |
| h3 | Istanbul, `oxc-coverage-instrument` | 0.77 s | 0.05 s | 0.03 s | 0.08 s | 0.93 s |
| h3 | V8 | 0.13 s | 0.18 s | 0.01 s | 0.09 s | 0.40 s |

The untested files step instruments files too, so its time is also part of the
instrumentation table above. The remap with `oxc-coverage-instrument` takes
longer for es-toolkit and hono (0.57 s against 0.35 s, and 0.32 s against
0.25 s). The cause is not measured yet.

### Whole test run

Wall time of `vitest run` (median). The coverage overhead is the time above the
run without coverage:

| Project | Test files | No coverage | Istanbul, Vitest's instrumenter | Istanbul, `oxc-coverage-instrument` | V8 | Overhead, Vitest's instrumenter | Overhead, `oxc-coverage-instrument` |
|:--|--:|--:|--:|--:|--:|--:|--:|
| es-toolkit | 720 | 185.0 s | 197.6 s | 194.3 s | 199.6 s | 12.6 s | 9.3 s |
| date-fns | 267 | 7.7 s | 37.4 s | 35.1 s | 9.7 s | 29.7 s | 27.4 s |
| hono | 122 | 22.9 s | 29.4 s | 26.3 s | 26.7 s | 6.5 s | 3.4 s |
| h3 | 70 | 14.3 s | 19.4 s | 17.3 s | 16.7 s | 5.1 s | 3.0 s |

### Coverage

The two strict Istanbul runs give the same covered and total counts for lines,
statements, functions, and branches in all four projects. For example, hono:
6265/6483 lines, 6495/6720 statements, 1089/1135 functions, and 4112/4408
branches in both runs. The V8 provider counts a little differently, as usual.

The `oxc-coverage-instrument-default` mode uses the default profile. Against
Vitest's instrumenter it records more branch entries, mostly optional chains,
and gives more functions a name:

| Project | Branch entries | Optional-chain entries | Branch paths that no test ran | Functions | Functions without a name | Branch coverage |
|:--|--:|--:|--:|--:|--:|--:|
| es-toolkit | 1609 → 1668 | 0 → 59 | 54 → 68 | 1161 | 332 → 274 | 98.4% → 98.0% |
| date-fns | 776 → 1064 | 0 → 287 | 211 → 254 | 572 | 252 → 45 | 88.4% → 89.4% |
| hono | 2143 → 2375 | 0 → 176 | 296 → 385 | 1135 | 1055 → 414 | 93.3% → 92.1% |
| h3 | 1302 → 1491 | 0 → 173 | 195 → 276 | 641 | 268 → 157 | 92.7% → 91.0% |

### What this means

- **The instrumentation step is 5.7x to 13.4x faster.** That step is where
  `oxc-coverage-instrument` works, and the coverage numbers stay the same.
- **In Node.js tests, the coverage overhead drops by 26% to 48%.** hono goes
  from 6.5 s to 3.4 s of overhead, h3 from 5.1 s to 3.0 s, and es-toolkit from
  12.6 s to 9.3 s. For es-toolkit that is 3.3 s of a 185 s run, which is close
  to the difference between two runs.
- **The steps after the test run are a small part.** They take 0.9 s to 3.7 s.
  Most of it is reading and merging the coverage that the test files wrote.
  The remap and the reports take 0.1 s to 1.1 s together, so a faster remap or
  faster reports inside Vitest would save at most about 1 s per run.
- **Istanbul with `oxc-coverage-instrument` is about as fast as V8 in Node.js
  tests**: a little faster than V8 for es-toolkit and hono, a little slower for
  h3. These differences are smaller than the difference between two runs. So
  in Node.js tests Istanbul coverage costs about as much as V8 coverage, and it
  keeps its exact branch and function counts.
- **In browser mode the instrumenter is a small part of the overhead.** For
  date-fns, Istanbul coverage adds 29.7 s, of which 4.0 s is instrumentation;
  `oxc-coverage-instrument` takes 2.3 s off that. Most of the rest goes to
  running the instrumented code and collecting the counters in the browser. V8
  coverage is much faster there, but it works only in Chromium.
- **The default profile finds more untested cases.** It reports 14 to 89 more
  branch paths that no test ran. Most of its extra branch entries are optional
  chains.
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
