// Compares two V8-to-Istanbul converters on the same real V8 coverage:
//
// - the converter inside monocart-coverage-reports (MCR add() of V8 data,
//   then generate() with an Istanbul report), and
// - v8ToIstanbul() of oxc-coverage-instrument.
//
// The input is the V8 precise coverage of the lodash 4.17.21 build under the
// workload in lodash.mjs. The reference is the Istanbul instrumentation of the
// same workload with the strict `istanbul` profile, which matches
// istanbul-lib-instrument. The script prints totals and median times. It
// does not fail on a difference, because the two converters measure V8 data
// differently by design.
//
// Usage: node scripts/v8-compare.mjs [--runs=5]

import inspector from 'node:inspector';
import { dirname, resolve } from 'node:path';
import { performance } from 'node:perf_hooks';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { promisify } from 'node:util';
import vm from 'node:vm';
import { MCR } from 'monocart-coverage-reports';
import { instrument, summarizeCoverage, v8ToIstanbul } from 'oxc-coverage-instrument';

import { exercise, LODASH_PATH, readLodash, runInContext } from './lodash.mjs';
import { METRICS } from './monocart.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const runsArg = process.argv.slice(2).find((arg) => arg.startsWith('--runs='));
const RUNS = runsArg ? Number(runsArg.slice('--runs='.length)) : 5;
if (!Number.isInteger(RUNS) || RUNS < 1) throw new Error(`invalid --runs value: ${runsArg}`);

const source = readLodash();
const url = pathToFileURL(LODASH_PATH).href;

const collectV8 = async () => {
  const session = new inspector.Session();
  const post = promisify(session.post).bind(session);
  session.connect();
  try {
    await post('Profiler.enable');
    await post('Profiler.startPreciseCoverage', { callCount: true, detailed: true });
    const context = vm.createContext({});
    vm.runInContext(source, context, { filename: url });
    exercise(context);
    const { result } = await post('Profiler.takePreciseCoverage');
    const script = result.find((entry) => entry.url === url);
    if (!script) throw new Error(`no V8 coverage for ${url}`);
    return script.functions;
  } finally {
    await post('Profiler.stopPreciseCoverage').catch(() => undefined);
    await post('Profiler.disable').catch(() => undefined);
    session.disconnect();
  }
};

const median = (values) => {
  const sorted = [...values].sort((a, b) => a - b);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
};

const time = async (fn) => {
  const samples = [];
  let result;
  for (let run = 0; run < RUNS; run += 1) {
    const start = performance.now();
    result = await fn();
    samples.push(performance.now() - start);
  }
  return { result, ms: median(samples) };
};

const totalsOf = (summary) =>
  Object.fromEntries(METRICS.map((metric) => [metric, { covered: summary[metric].covered, total: summary[metric].total }]));

const runMonocart = async (functions, outputDir) => {
  const mcr = MCR({ outputDir, reports: ['json'], logging: 'error' });
  mcr.cleanCache();
  await mcr.add([{ url, source, functions }]);
  const results = await mcr.generate();
  return results.summary;
};

const functions = await collectV8();
const functionsJson = JSON.stringify(functions);

// Reference: Istanbul instrumentation of the same workload.
const { code } = instrument(source, LODASH_PATH, { compat: 'istanbul' });
const instrumented = runInContext(code, LODASH_PATH);
exercise(instrumented);
const reference = totalsOf(summarizeCoverage(JSON.stringify(instrumented.__coverage__)).total);

// Warm both converters once, outside the timed runs.
v8ToIstanbul(source, LODASH_PATH, functionsJson);
await runMonocart(functions, resolve(root, 'coverage', 'v8-monocart'));

const oxc = await time(() => v8ToIstanbul(source, LODASH_PATH, functionsJson));
const monocart = await time(() => runMonocart(functions, resolve(root, 'coverage', 'v8-monocart')));

const oxcTotals = totalsOf(summarizeCoverage(JSON.stringify({ [LODASH_PATH]: JSON.parse(oxc.result) })).total);
const monocartTotals = totalsOf(monocart.result);
const format = ({ covered, total }) => `${covered}/${total} (${total ? ((covered / total) * 100).toFixed(2) : '100.00'}%)`;

console.log(`lodash 4.17.21, ${functions.length} V8 function entries, median of ${RUNS} runs\n`);
console.table(
  METRICS.map((metric) => ({
    metric,
    'Istanbul instrumentation': format(reference[metric]),
    'oxc-coverage-instrument v8ToIstanbul': format(oxcTotals[metric]),
    'monocart converter': format(monocartTotals[metric]),
  })),
);
console.table([
  { converter: 'oxc-coverage-instrument v8ToIstanbul()', 'median ms': oxc.ms.toFixed(1) },
  { converter: 'monocart add() + generate() with the json report', 'median ms': monocart.ms.toFixed(1) },
]);
