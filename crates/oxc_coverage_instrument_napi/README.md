# oxc-coverage-instrument for Node.js

Istanbul-compatible JavaScript and TypeScript coverage instrumentation, built on
the [Oxc](https://oxc.rs) parser and exposed to Node.js through napi-rs.

> **Unofficial.** Independent community project built on Oxc crates. Not part
> of, affiliated with, or endorsed by the Oxc project.

## Overview

The `oxc-coverage-instrument` npm package wraps the native instrumenter. It
exposes instrumentation, coverage remapping, V8-to-Istanbul conversion, coverage
summaries, and the Istanbul reporters, plus ready-made adapters for Vitest's
Istanbul provider, for `vite-plugin-istanbul`, and for `karma-coverage`.

```bash
npm install oxc-coverage-instrument
```

```javascript
import { instrument } from 'oxc-coverage-instrument';

const result = instrument(source, 'file.js', {
  coverageVariable: '__coverage__',
  sourceMap: true,
});

result.code;                                         // instrumented source
const coverageMap = JSON.parse(result.coverageMap);  // Istanbul format
result.sourceMap;                                    // source map JSON, when enabled
```

Pass `{ compat: 'istanbul' }` when replacing an existing
`istanbul-lib-instrument` instance without changing coverage denominators or
metadata shape. The profile disables Oxc's logical-assignment and optional-chain
branches, uses Istanbul's anonymous function names and method spans, and emits
its empty synthetic `else` locations. Without the profile, existing Oxc
extensions remain enabled.

## Key Features

### Vitest

Requires `vitest` and `@vitest/coverage-istanbul` 4.1.5 or newer, which is where
the `coverage.instrumenter` option was added.

```typescript
import { defineConfig } from 'vitest/config'
import { createOxcInstrumenter } from 'oxc-coverage-instrument/vitest'

export default defineConfig({
  test: {
    coverage: {
      provider: 'istanbul',
      instrumenter: (options) => createOxcInstrumenter(options),
    }
  }
})
```

The factory forwards `coverageVariable` and `ignoreClassMethods` to the native
instrumenter. Collection, merging, and reporting in the Istanbul provider are
untouched.

The adapter auto-detects raw TypeScript: when the filename ends in `.ts`,
`.tsx`, `.mts`, or `.cts` and no `inputSourceMap` was supplied, it runs an
in-process TypeScript-strip pass, so the output is executable JavaScript and the
coverage map points at the original `.ts` source. No `@babel/preset-typescript`
or upstream `tsc` step is needed.

Set `stripTypescript: false` when running under a toolchain that pre-transforms
TypeScript without emitting an `inputSourceMap`
(`@vitejs/plugin-react-swc` in some configurations, Bun's TypeScript runner,
Node 23 and newer with `--experimental-strip-types`). Set `stripTypescript: true`
to force the strip pass regardless of filename or `inputSourceMap`.

See `examples/vitest-typescript/` in the repository for a working project.

### Legacy decorators

By default, decorator syntax (Stage 3 and legacy `experimentalDecorators` alike)
flows through the strip pass verbatim. Counters land on the surrounding class
bodies and methods, and a downstream tool (Babel, `tsc`, SWC, or native Node
decorators) lowers them at runtime.

Set `experimentalDecorators: true` on the instrument call, or on
`createOxcInstrumenter`, to lower legacy decorators in-process into
`_decorate(...)` calls. Add `emitDecoratorMetadata: true` to also emit
`_decorateMetadata("design:type", ...)`,
`_decorateMetadata("design:paramtypes", ...)`, and
`_decorateMetadata("design:returntype", ...)`. NestJS dependency injection,
TypeORM column type inference, and class-validator all read that metadata.

`createOxcInstrumenter` promotes `emitDecoratorMetadata: true` to also enable
`experimentalDecorators`, matching `tsconfig.json` semantics. The bare
`instrument()` binding rejects that combination with an `Error` instead, so the
underlying Rust `DecoratorMode` keeps invalid states unrepresentable.

With `emitDecoratorMetadata` on, `strictNullChecks` decides how a nullable union
is written into `design:type`, mirroring the `tsconfig.json` flag of the same
name. It defaults to `true`, matching `tsc` under `strict`, so `foo: string |
null` emits `Object`. Set it to `false` if your sources are compiled without
`strictNullChecks`: `null` and `undefined` are then elided from the union first
and the same property emits `String`. A mismatch is silent. The instrumented code
runs either way, but the metadata consumers above resolve a different type than
`tsc` produced.

Lowered output imports helpers from
[`@oxc-project/runtime`](https://www.npmjs.com/package/@oxc-project/runtime):

```bash
npm install @oxc-project/runtime
```

`Cannot find module '@oxc-project/runtime/helpers/decorate'` at test time means
the package is missing from the consumer. Install it as a regular dependency,
not a dev dependency, so it is present wherever the instrumented code runs. The
relationship mirrors `babel-plugin-transform-typescript-metadata` and
`@babel/runtime` in a Babel pipeline.

### vite-plugin-istanbul

Requires `vite-plugin-istanbul` 9.0.0 or newer, which is where the
`instrumenter` option was added.

```typescript
import { defineConfig } from 'vite'
import istanbul from 'vite-plugin-istanbul'
import { createOxcInstrumenter } from 'oxc-coverage-instrument/vitest'

export default defineConfig({
  plugins: [
    istanbul({
      instrumenter: createOxcInstrumenter(),
      include: ['src/**/*.{js,ts,jsx,tsx}'],
      exclude: ['node_modules', 'test/'],
    }),
  ],
})
```

### Karma

Requires `karma-coverage` 2. The `oxc-coverage-instrument/karma` subpath
exports an `Instrumenter` class with the shape that
`coverageReporter.instrumenters` expects: `instrument(code, filename,
callback, inputSourceMap)`, `instrumentSync()`, `lastSourceMap()`, and
`lastFileCoverage()`.

```javascript
// karma.conf.js
module.exports = (config) => {
  config.set({
    preprocessors: { 'src/**/*.js': ['coverage'] },
    reporters: ['progress', 'coverage'],
    coverageReporter: {
      instrumenters: { oxc: require('oxc-coverage-instrument/karma') },
      // karma-coverage matches these patterns against the absolute path.
      instrumenter: { '**/*.js': 'oxc' },
      instrumenterOptions: { oxc: { compat: 'istanbul' } },
    },
  });
};
```

`instrumenterOptions.oxc` accepts the options of `createOxcInstrumenter()`,
for example `compat`, `coverageVariable`, `ignoreClassMethods`, and
`reportLogic`. Keep the default `coverageVariable` under Karma. The
`karma-jasmine` adapter sends only `window.__coverage__` to the reporter, so a
different variable gives an empty report. The istanbul instrumenter has the
same limit. The adapter does not use the Babel code-generation options that
`karma-coverage` adds, for example `autoWrap` and `codeGenerationOptions`.
`lastSourceMap()` returns a map only when `karma-coverage` sets
`produceSourceMap`, which it does when the input file has a source map. An
input source map composes into the output map and stays on the file coverage,
so the reports point at the original source.

With `compat: 'istanbul'`, the statement, function, and branch maps and the
hit counts are the same as with the default instrumenter of `karma-coverage`. The
[Karma example](../../examples/karma/) checks this in headless Chrome.

### Storybook

`@storybook/addon-coverage` gives its `istanbul` options to
`vite-plugin-istanbul` on the Vite builder. Version 3.0.2 of the addon depends
on `vite-plugin-istanbul` 6, which has no `instrumenter` option. Install
version 9 and make the addon use it with an override:

```bash
npm install -D @storybook/addon-coverage vite-plugin-istanbul@^9 oxc-coverage-instrument
```

```json
{
  "overrides": {
    "vite-plugin-istanbul": "$vite-plugin-istanbul"
  }
}
```

With pnpm, use `pnpm.overrides`. With Yarn, use `resolutions`. Then give the
instrumenter to the addon:

```typescript
// .storybook/main.ts
import type { StorybookConfig } from '@storybook/react-vite'
import { createOxcInstrumenter } from 'oxc-coverage-instrument/vitest'

const config: StorybookConfig = {
  framework: '@storybook/react-vite',
  stories: ['../src/**/*.stories.tsx'],
  addons: [
    {
      name: '@storybook/addon-coverage',
      options: {
        istanbul: { instrumenter: createOxcInstrumenter({ compat: 'istanbul' }) },
      },
    },
  ],
}

export default config
```

`test-storybook --coverage` then collects coverage from the
`oxc-coverage-instrument` counters. The
[`examples/storybook-vite`](https://github.com/fallow-rs/oxc-coverage/tree/main/examples/storybook-vite)
project compares this coverage with the coverage of `istanbul-lib-instrument`.
The webpack builder does not accept an instrumenter yet: the addon makes its
own `istanbul-lib-instrument` instance there.

### Custom Vite or Rollup plugin

```javascript
import { instrument } from 'oxc-coverage-instrument';

export function coveragePlugin() {
  return {
    name: 'coverage-instrument',
    transform(code, id) {
      if (process.env.COVERAGE && /\.[jt]sx?$/.test(id) && !id.includes('node_modules')) {
        const result = instrument(code, id, { sourceMap: true });
        return { code: result.code, map: result.sourceMap ? JSON.parse(result.sourceMap) : undefined };
      }
    },
  };
}
```

### Remapping coverage

`remapCoverageMap` walks a coverage map through its embedded `inputSourceMap`.
Pass `{ dropUnmapped: true }` to drop statement, function, and branch entries that
fail to remap along with their `s`, `f`, `b`, and `bT` slots:

```js
import { remapCoverageMap } from 'oxc-coverage-instrument';

const remapped = JSON.parse(
    remapCoverageMap(JSON.stringify(coverageMap), { dropUnmapped: true }),
);
```

Set `composeInputSourceMap: true` alongside `inputSourceMap` on the `instrument`
call to fold the input map in eagerly instead. The coverage map is then keyed by
the original source path, carries original-source positions, and has no
`inputSourceMap`, so a later `remapCoverageMap` is a no-op.

### Summaries and reports

`summarizeCoverage` returns the total and per-file metrics of a coverage map.
The numbers match `istanbul-lib-coverage`, including the truncation of `pct` to
two decimals:

```js
import { summarizeCoverage } from 'oxc-coverage-instrument';

const { total, files } = summarizeCoverage(JSON.stringify(coverageMap));
total.lines.pct; // for example 66.66
```

`createReport` renders the `text`, `text-summary`, `json-summary`, `lcov`, or
`cobertura` report and returns it as a string. Pass `rootDir` to make the `lcov`
and `cobertura` source paths relative; without it, paths stay as the coverage map
records them. Pass `process.cwd()` to match `istanbul-reports`:

```js
import { createReport } from 'oxc-coverage-instrument';
import { writeFileSync } from 'node:fs';

const json = JSON.stringify(coverageMap);
writeFileSync('coverage/lcov.info', createReport(json, 'lcov', { rootDir: process.cwd() }));
console.log(createReport(json, 'text'));
```

`writeHtmlReport` writes the html report into a directory and creates it when
needed. It reads the source files from disk, remaps entries that carry an
`inputSourceMap`, and accepts a `greenThreshold` in `[0, 100]` (default `80`):

```js
import { writeHtmlReport } from 'oxc-coverage-instrument';

writeHtmlReport(JSON.stringify(coverageMap), 'coverage/html', { rootDir: process.cwd() });
```

The html reporter is only in the native bindings. The WebAssembly binding returns
an error for `writeHtmlReport` and keeps the other reports.

`createReport` and `summarizeCoverage` do not remap. Call `remapCoverageMap`
first when the map carries an `inputSourceMap`.

## Architecture

The package is a napi-rs binding over `oxc_coverage_instrument`,
`oxc_coverage_report`, and `oxc_coverage_reports`; all instrumentation,
remapping, conversion, and reporting logic lives in Rust, and the
JavaScript layer only marshals options and JSON. Coverage maps cross the boundary
as JSON strings rather than as object graphs, which keeps one serialization
format between the native and WebAssembly bindings.

### Runtime matrix

The npm package ships prebuilt native bindings for seven platforms plus two
WebAssembly fallbacks: `@oxc-coverage-instrument/binding-wasm32-wasi` for
runtimes with `SharedArrayBuffer`, and a single-threaded `wasm32-wasip1` variant
for runtimes without it. The WASM binding is selected when no matching native
binary is available, or explicitly through `NAPI_RS_FORCE_WASI`.

| Runtime | Binding | Notes |
|:--------|:--------|:------|
| Node 18+ (darwin/linux/win32, x64/arm64) | native | Preferred when a matching `.node` is installed. |
| Node 22 LTS (other architectures) | wasm | Loads via `node:wasi` and emits an experimental-feature warning. |
| Deno 2.x | wasm | Uses Deno's `node:wasi` polyfill. Live deploy smoke tracked in [#88](https://github.com/fallow-rs/oxc-coverage/issues/88). |
| Browser with COOP/COEP | threaded wasm | Imports the `browser` export and uses the `wasm32-wasip1-threads` binding when `SharedArrayBuffer` is available. Requires `Cross-Origin-Opener-Policy: same-origin`, `Cross-Origin-Embedder-Policy: require-corp`, and an ESM bundler with top-level await. webpack 4 and Parcel 1 are not supported. |
| Browser without COOP/COEP | single-threaded wasm | Falls back to the `wasm32-wasip1` binding. |
| Bun (supported targets) | native | Bun's `node:wasi` is incomplete ([oven-sh/bun#16156](https://github.com/oven-sh/bun/issues/16156)), so the native binding is used. |
| Cloudflare Workers | single-threaded wasm | Workers has no `SharedArrayBuffer`; the `browser` export selects `wasm32-wasip1`. |
| StackBlitz / WebContainer | partial | Newer WebContainer images load the wasm binding; older ones may fall through. Live smoke tracked in [#88](https://github.com/fallow-rs/oxc-coverage/issues/88). |

`examples/wasm-node/` is an end-to-end Node smoke exercising native and WASM.
`examples/cloudflare-workers/` compares Worker `FileCoverage` with native output
byte for byte, without credentials.

`NAPI_RS_FORCE_WASI` opts out of the native binding even when one is available.
Set it to `1` to prefer WASM and fall back to native if the WASM binding cannot
load, or to `error` to fail loudly instead of falling back. The variable is read
by the napi-rs 3 loader, not by this package.

Instrumentation semantics, the option reference, and the documented divergences
from `istanbul-lib-instrument` are shared with the Rust crate and live in
[its README](https://github.com/fallow-rs/oxc-coverage/blob/main/crates/oxc_coverage_instrument/README.md).
