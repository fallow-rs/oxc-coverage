# Vitest monorepo example

Coverage for a workspace with several Vitest projects, with the oxc
instrumenter set once at the root.

## What this shows

- `test.projects` in the root `vitest.config.ts` lists two packages,
  `packages/core` and `packages/api`.
- `test.coverage` lives at the root, not in the projects. Vitest uses one
  coverage provider for all projects, so one `instrumenter` line covers the
  whole workspace, and `coverage-final.json` holds the files of both packages.
- `packages/api` imports `packages/core`. A file that several projects load is
  instrumented and counted as one entry.

## Run

```bash
cd examples/vitest-monorepo
npm install
npm run coverage
npm run verify
```

`npm run verify` checks that one report holds `slug.ts` and `routes.ts`, that
the class methods `create` and `find` are named in the function map, and that
the loop in `uniqueSlug` was counted.
