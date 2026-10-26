import { defineConfig } from 'vitest/config';
import { createOxcInstrumenter } from 'oxc-coverage-instrument/vitest';

// Coverage is configured once, at the root. Every project below uses the same
// provider and instrumenter, and Vitest merges their coverage into one report.
export default defineConfig({
  test: {
    projects: [
      { test: { name: 'core', root: './packages/core', environment: 'node' } },
      { test: { name: 'api', root: './packages/api', environment: 'node' } },
    ],
    coverage: {
      provider: 'istanbul',
      instrumenter: (options) => createOxcInstrumenter(options),
      include: ['packages/*/src/**/*.ts'],
      exclude: ['**/*.test.ts'],
      reporter: ['json', 'text'],
    },
  },
});
