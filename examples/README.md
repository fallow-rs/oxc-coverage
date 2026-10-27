# Examples

Each example is a small, runnable project. The Vitest examples use the
Istanbul coverage provider with the `oxc-coverage-instrument` instrumenter.

| Example | Shows |
|:--|:--|
| [`vitest-typescript`](vitest-typescript/) | The basic setup: TypeScript sources, coverage keyed by the `.ts` files. |
| [`vitest-react`](vitest-react/) | React components in TSX; coverage points at the `.tsx` source. |
| [`vitest-monorepo`](vitest-monorepo/) | Several Vitest projects with one root coverage config and one report. |
| [`vitest-ci-reports`](vitest-ci-reports/) | `lcov`, `cobertura`, HTML, and a coverage threshold for CI, from the npm API. |
| [`vitest-real-projects`](vitest-real-projects/) | Measured speed and coverage parity on es-toolkit, date-fns, hono, and h3. |
| [`wasm-node`](wasm-node/) | The WebAssembly binding in Node.js. |
| [`cloudflare-workers`](cloudflare-workers/) | The instrumenter inside a Cloudflare Worker. |

## Use it in your own Vitest project

Requires `vitest` and `@vitest/coverage-istanbul` 4.1.5 or newer.

```bash
npm install -D @vitest/coverage-istanbul oxc-coverage-instrument
```

Add one line to the coverage config:

```ts
import { defineConfig } from 'vitest/config';
import { createOxcInstrumenter } from 'oxc-coverage-instrument/vitest';

export default defineConfig({
  test: {
    coverage: {
      provider: 'istanbul',
      instrumenter: (options) => createOxcInstrumenter({ ...options, compat: 'istanbul' }),
    },
  },
});
```

Everything else stays as it is: `include`, `exclude`, `reporter`,
`thresholds`, Vitest projects, and browser mode. Only the step that inserts
the coverage counters changes.

`compat: 'istanbul'` keeps the exact numbers of Vitest's default
instrumenter. Without `compat`, the instrumenter adds branch entries for
`??=`, `||=`, `&&=`, and optional chains, and names more functions. That shows
more untested cases, but your branch percentage can change. The crate README
lists each difference.

## Where it is faster

`oxc-coverage-instrument` replaces the step where the Istanbul provider inserts
coverage counters into each source file. On four real projects
([`vitest-real-projects`](vitest-real-projects/), measured on a GitHub-hosted
runner) that step is 5.7x to 13.4x faster, and the coverage numbers stay the
same:

| Project | Files instrumented | Vitest's instrumenter | `oxc-coverage-instrument` | Speedup |
|:--|--:|--:|--:|--:|
| h3 | 81 | 1.5 s | 0.1 s | 13.4x |
| hono | 636 | 11.8 s | 0.9 s | 13.3x |
| date-fns | 1061 | 4.0 s | 0.5 s | 7.5x |
| es-toolkit | 802 | 5.7 s | 1.0 s | 5.7x |

The times are rounded; the speedup comes from the unrounded times. The rest of
a test run (running the tests and writing the report) takes the same time as
before.

Vitest 5 caches transformed modules on disk. A rerun with a warm cache does not
instrument the files again, so the gain is largest on a cold run, which is the
usual case in CI.
