# Examples

Each example is a small, runnable project. The Vitest examples use the
Istanbul coverage provider with the oxc instrumenter.

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
      instrumenter: (options) => createOxcInstrumenter(options),
    },
  },
});
```

Everything else stays as it is: `include`, `exclude`, `reporter`,
`thresholds`, Vitest projects, and browser mode. Only the step that inserts
the coverage counters changes.

To keep the exact numbers of Vitest's default instrumenter, pass the strict
Istanbul profile:

```ts
instrumenter: (options) => createOxcInstrumenter({ ...options, compat: 'istanbul' }),
```

Without `compat`, the instrumenter adds a few extra branch entries (for
`??=`, `||=`, `&&=`, and optional chains) and names more functions. The crate
README lists each difference.

## Where it is faster

The oxc instrumenter replaces the step where the Istanbul provider inserts
coverage counters into each source file. On four real projects
([`vitest-real-projects`](vitest-real-projects/), measured on a GitHub-hosted
runner) that step is 5.6x to 11.6x faster, and the coverage numbers stay the
same:

| Project | Files instrumented | Vitest's instrumenter | oxc | Speedup |
|:--|--:|--:|--:|--:|
| hono | 636 | 10.7 s | 0.9 s | 11.6x |
| h3 | 81 | 1.4 s | 0.1 s | 11.6x |
| date-fns | 1061 | 3.9 s | 0.5 s | 7.2x |
| es-toolkit | 802 | 5.5 s | 1.0 s | 5.6x |

The times are rounded; the speedup comes from the unrounded times. The rest of
a test run (running the tests and writing the report) takes the same time as
before.

Vitest 5 caches transformed modules on disk. A rerun with a warm cache does not
instrument the files again, so the gain is largest on a cold run, which is the
usual case in CI.
