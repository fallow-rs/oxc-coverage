// Instruments the real lodash build with oxc-coverage-instrument, runs the
// workload, and gives the collected __coverage__ to monocart-coverage-reports.
// It does this for the strict `istanbul` profile and for the default profile.
// The default profile adds branch types and `x_` fields that Istanbul does
// not write. The script exits with code 1 when the monocart totals differ from
// the totals of oxc-coverage-instrument.

import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { instrument } from 'oxc-coverage-instrument';

import { exercise, LODASH_PATH, readLodash, runInContext } from './lodash.mjs';
import { compareTotals, generateMonocartReports, reportComparison } from './monocart.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const source = readLodash();

const profiles = [
  { name: 'istanbul', options: { compat: 'istanbul' } },
  { name: 'default', options: { functionIdentityOverlay: true } },
];

for (const { name, options } of profiles) {
  const { code } = instrument(source, LODASH_PATH, options);
  const context = runInContext(code, LODASH_PATH);
  exercise(context);
  const coverage = context.__coverage__;
  const json = JSON.stringify(coverage);
  const summary = await generateMonocartReports(
    JSON.parse(json),
    resolve(root, 'coverage', `lodash-${name}`),
    ['json-summary', 'lcovonly', 'html'],
  );
  reportComparison(`lodash 4.17.21, ${name} profile`, compareTotals(summary, json));
}
