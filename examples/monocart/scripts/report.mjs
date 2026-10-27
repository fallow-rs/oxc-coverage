// Gives coverage/coverage-final.json from the Vitest run to
// monocart-coverage-reports and writes its reports to coverage/monocart.
// The script exits with code 1 when the monocart totals differ from the
// totals of oxc-coverage-instrument.

import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createReport } from 'oxc-coverage-instrument';

import { compareTotals, generateMonocartReports, reportComparison } from './monocart.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const json = readFileSync(resolve(root, 'coverage', 'coverage-final.json'), 'utf8');

const summary = await generateMonocartReports(JSON.parse(json), resolve(root, 'coverage', 'monocart'), [
  'html',
  'console-summary',
  'json-summary',
  'lcovonly',
]);

process.stdout.write('\noxc-coverage-instrument text-summary:\n');
process.stdout.write(createReport(json, 'text-summary'));
reportComparison('Vitest coverage-final.json', compareTotals(summary, json));
