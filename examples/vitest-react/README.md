# Vitest + React example

Coverage for React components written in TSX, with the `oxc-coverage-instrument` instrumenter in
Vitest's Istanbul provider.

## What this shows

- `vitest.config.ts` keeps Vite's own JSX transform (`oxc.jsx`) and adds one
  line to the coverage config: `instrumenter: (options) => createOxcInstrumenter(options)`.
- Vite compiles the TSX to JavaScript first. The instrumenter reads that output
  and the source map Vite passes along, so `coverage-final.json` is keyed by
  `src/PriceTag.tsx` and every entry points at a line of the `.tsx` file.
- The tests render with `react-dom/server`, so no DOM environment is needed.
  With `@testing-library/react`, set `environment: 'jsdom'` or
  `'happy-dom'`; the coverage config stays the same.

## Run

```bash
cd examples/vitest-react
npm install
npm run coverage
npm run verify
```

`npm run verify` checks that the coverage is keyed by the `.tsx` source, that
`formatAmount` and `PriceTag` are in the function map, and that the tests hit
both arms of each discount conditional.
