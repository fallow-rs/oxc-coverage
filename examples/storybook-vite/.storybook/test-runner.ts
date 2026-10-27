import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import type { TestRunnerConfig } from '@storybook/test-runner';

// The test-runner merges the coverage of all stories with `nyc merge`, and nyc
// remaps the result through the source maps. The compare script also needs
// the coverage before that remap. When RAW_COVERAGE_DIR is set, this hook
// writes `window.__coverage__` to that folder after each story. The
// test-runner reads the same object directly after this hook.
const rawCoverageDir = process.env.RAW_COVERAGE_DIR;

const config: TestRunnerConfig = {
  async postVisit(page, context) {
    if (!rawCoverageDir) return;
    const coverage = await page.evaluate(() => (window as { __coverage__?: unknown }).__coverage__);
    if (!coverage) return;
    mkdirSync(rawCoverageDir, { recursive: true });
    writeFileSync(join(rawCoverageDir, `${context.id}.json`), JSON.stringify(coverage));
  },
};

export default config;
