import { defineConfig } from 'vitest/config';
import { createOxcInstrumenter } from 'oxc-coverage-instrument/vitest';

// Vitest writes only coverage-final.json. scripts/report.mjs turns that file
// into the CI reports with the reporters of oxc-coverage-instrument.
export default defineConfig({
  test: {
    coverage: {
      provider: 'istanbul',
      instrumenter: (options) => createOxcInstrumenter(options),
      include: ['src/**/*.ts'],
      exclude: ['src/**/*.test.ts'],
      reporter: ['json'],
    },
  },
});
