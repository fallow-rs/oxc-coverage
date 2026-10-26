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
runner) that step is 5.6x to 11.6x faster, the coverage numbers stay the same,
and the coverage overhead of a Node.js test run drops by 30% to 52%:

| Project | `vitest run`, no coverage | Istanbul, Vitest default | Istanbul + oxc |
|:--|--:|--:|--:|
| hono | 21.0 s | 27.3 s | 24.0 s |
| h3 | 13.2 s | 18.5 s | 16.4 s |
| es-toolkit | 172.8 s | 185.7 s | 181.7 s |

In browser mode the gain is smaller, because collecting the counters in the
browser costs more than inserting them.

Vitest 5 caches transformed modules on disk. A rerun with a warm cache does not
instrument the files again, so the gain is largest on a cold run, which is the
usual case in CI.
