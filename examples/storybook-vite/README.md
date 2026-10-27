# Storybook + Vite example

Story coverage with `@storybook/addon-coverage` on the Vite builder, with
`oxc-coverage-instrument` as the instrumenter. The Storybook test-runner runs
the stories and collects the coverage.

## What this shows

- `.storybook/main.ts` gives `istanbul.instrumenter` to the addon. The addon
  gives its `istanbul` options to `vite-plugin-istanbul` without changes.
- `vite-plugin-istanbul` accepts `instrumenter` from version 9.0.0.
  `@storybook/addon-coverage` 3.0.2 depends on version 6. The `overrides`
  entry in `package.json` makes the addon use the version 9 that this project
  installs. Without the override, version 6 ignores `instrumenter` and uses
  `istanbul-lib-instrument`.
- `compat: 'istanbul'` keeps the same coverage shape as
  `istanbul-lib-instrument`.
- `npm run compare` builds the Storybook two times: one time with
  `COVERAGE_INSTRUMENTER=istanbul` (the default `istanbul-lib-instrument`) and
  one time with `oxc-coverage-instrument`. For each build it runs
  `test-storybook --coverage` and compares the coverage of the two builds at
  two points:
  - Raw coverage, before the remap. The `postVisit` hook in
    `.storybook/test-runner.ts` writes `window.__coverage__` after each story.
    The positions are in the instrumented code, and each entry has the
    `inputSourceMap` from Vite. The script also compares these source maps.
  - Remapped coverage. The test-runner merges the stories with `nyc merge`, and
    nyc remaps the result to the `.tsx` sources. The result is
    `coverage/<instrumenter>/coverage-storybook.json`.
- The script also remaps the raw coverage with the `istanbul-lib-source-maps`
  copy that nyc uses. It checks that this result has the same maps and the
  same covered entries as the nyc output. This shows that the raw coverage is
  the data that nyc remapped.

## Run

Requires the Playwright Chromium browser
(`npx playwright install chromium`).

```bash
cd examples/storybook-vite
npm install
npm run compare
```

The script fails when a file key, a statement, function, or branch map, or
the covered state of an entry is different. It also fails when a raw entry has
no `inputSourceMap`, because then the entry is not raw coverage. The script
reports different hit counts, but it does not fail on them. One earlier run
showed different counts between the two builds, and the cause is not
confirmed. The test-runner also saves the coverage on the `beforeunload` event
of the page, so nyc adds more snapshots than the `postVisit` hook sees.

## Use it in your own Storybook

```bash
npm install -D @storybook/addon-coverage vite-plugin-istanbul@^9 oxc-coverage-instrument
```

Make the addon use `vite-plugin-istanbul` 9. With npm:

```json
{
  "overrides": {
    "vite-plugin-istanbul": "$vite-plugin-istanbul"
  }
}
```

With pnpm, put the same entry in `pnpm.overrides`. Yarn `resolutions` does
not accept a `$` reference, so give a version range:

```json
{
  "resolutions": {
    "vite-plugin-istanbul": "^9.0.1"
  }
}
```

```ts
// .storybook/main.ts
import type { StorybookConfig } from '@storybook/react-vite';
import { createOxcInstrumenter } from 'oxc-coverage-instrument/vitest';

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
};

export default config;
```

Then run `test-storybook --coverage` as before.

## Limits

- The webpack builder does not work yet. The addon makes its own
  `istanbul-lib-instrument` instance for webpack and does not accept an
  instrumenter from its options.
- With `@storybook/addon-vitest`, Vitest collects the coverage, not this
  addon. Use the Vitest setup in the [examples README](../README.md) for that.
