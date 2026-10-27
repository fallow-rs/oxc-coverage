import { defineConfig } from 'vitest/config';
import { createOxcInstrumenter } from 'oxc-coverage-instrument/vitest';

// Vitest writes only coverage-final.json. scripts/report.mjs gives that file
// to monocart-coverage-reports as Istanbul data. The default profile is used
// on purpose: it adds `optional-chain` and `??` branch entries, and the
// example shows that monocart accepts them.
export default defineConfig({
  test: {
    include: ['src/**/*.test.ts'],
    coverage: {
      provider: 'istanbul',
      instrumenter: (options) => createOxcInstrumenter(options),
      include: ['src/**/*.ts'],
      exclude: ['src/**/*.test.ts'],
      reporter: ['json'],
    },
  },
});
