import type { StorybookConfig } from '@storybook/react-vite';
import { createOxcInstrumenter } from 'oxc-coverage-instrument/vitest';

// COVERAGE_INSTRUMENTER=istanbul keeps the default istanbul-lib-instrument.
// The compare script uses it to build the reference coverage.
const useIstanbul = process.env.COVERAGE_INSTRUMENTER === 'istanbul';

const config: StorybookConfig = {
  framework: '@storybook/react-vite',
  stories: ['../src/**/*.stories.tsx'],
  addons: [
    {
      name: '@storybook/addon-coverage',
      options: {
        // The addon gives these options to vite-plugin-istanbul. Version 9 of
        // the plugin accepts an `instrumenter`; the `overrides` entry in
        // package.json makes the addon load version 9.
        istanbul: useIstanbul
          ? {}
          : { instrumenter: createOxcInstrumenter({ compat: 'istanbul' }) },
      },
    },
  ],
};

export default config;
