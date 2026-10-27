// Runs the Storybook test-runner with coverage two times: one time with the
// default istanbul-lib-instrument and one time with oxc-coverage-instrument.
// Then it compares the coverage of the two runs at two points:
//
// 1. Raw coverage: `window.__coverage__` after each story, before any remap.
//    The positions are in the instrumented code, and each entry has the
//    `inputSourceMap` from Vite. The `postVisit` hook in
//    .storybook/test-runner.ts writes these objects to coverage/<name>/raw.
// 2. Remapped coverage: coverage/<name>/coverage-storybook.json from the
//    test-runner. The test-runner merges the stories with `nyc merge`, and
//    nyc remaps the result through the source maps.
//
// The script also remaps the raw coverage with the istanbul-lib-source-maps
// copy that nyc uses, and checks that the result agrees with the nyc output.
// This shows that the raw coverage is the same data that nyc remapped. It fails when a map or
// the covered state of an entry differs.

import { spawn, spawnSync } from 'node:child_process';
import { existsSync, readdirSync, readFileSync, rmSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';
import libCoverage from 'istanbul-lib-coverage';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
// nyc 15 uses istanbul-lib-source-maps 4. Version 5 keeps an empty `else`
// location for an `if` branch, and version 4 removes it. Load the copy that
// nyc loads, so that the cross-check compares the same remap logic.
const require = createRequire(import.meta.url);
const libSourceMaps = createRequire(require.resolve('nyc/package.json'))('istanbul-lib-source-maps');
const bin = (name) => resolve(root, 'node_modules', '.bin', name);
const instrumenters = ['istanbul', 'oxc'];
const basePort = 6106;
const serverStartTimeoutMs = 30_000;
const expectedFiles = ['Counter.tsx', 'PriceTag.tsx'];

const fail = (message) => {
  console.error(`FAIL: ${message}`);
  process.exit(1);
};

const run = (command, args, env) => {
  const result = spawnSync(command, args, {
    cwd: root,
    stdio: 'inherit',
    // The example is inside a larger repository. Without
    // STORYBOOK_PROJECT_ROOT, the test-runner uses the repository root as the
    // Jest root and scans every file in the repository.
    env: { ...process.env, STORYBOOK_DISABLE_TELEMETRY: '1', STORYBOOK_PROJECT_ROOT: root, ...env },
  });
  if (result.status !== 0) fail(`${command} ${args.join(' ')} exited with ${result.status}`);
};

const waitForServer = async (url) => {
  const deadline = Date.now() + serverStartTimeoutMs;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(`${url}/index.json`);
      if (response.ok) return;
    } catch {
      // The server is not listening yet.
    }
    await new Promise((done) => setTimeout(done, 250));
  }
  fail(`no response from ${url} after ${serverStartTimeoutMs} ms`);
};

const rawDir = (instrumenter) => resolve(root, 'coverage', instrumenter, 'raw');

// Merges the per-story snapshots the same way as `nyc merge`, but without the
// remap. Each entry keeps its positions in the instrumented code and its
// `inputSourceMap`.
const readRaw = (instrumenter) => {
  const dir = rawDir(instrumenter);
  const files = existsSync(dir) ? readdirSync(dir).filter((name) => name.endsWith('.json')).sort() : [];
  if (files.length === 0) fail(`no raw coverage in ${dir}; check .storybook/test-runner.ts`);
  const map = libCoverage.createCoverageMap({});
  for (const name of files) map.merge(JSON.parse(readFileSync(resolve(dir, name), 'utf8')));
  // toJSON() gives FileCoverage objects. A JSON round trip gives plain data.
  return JSON.parse(JSON.stringify(map));
};

const collect = async (instrumenter, port) => {
  const outDir = resolve(root, 'dist', instrumenter);
  const coverageDir = resolve(root, 'coverage', instrumenter);
  rmSync(outDir, { recursive: true, force: true });
  rmSync(coverageDir, { recursive: true, force: true });

  console.log(`\n== ${instrumenter}: build Storybook ==`);
  run(bin('storybook'), ['build', '--quiet', '-o', outDir], { COVERAGE_INSTRUMENTER: instrumenter });

  const url = `http://127.0.0.1:${port}`;
  const server = spawn(bin('http-server'), [outDir, '-p', String(port), '-a', '127.0.0.1', '-s'], { cwd: root });
  try {
    await waitForServer(url);
    console.log(`== ${instrumenter}: run test-storybook --coverage ==`);
    run(bin('test-storybook'), ['--url', url, '--coverage', '--coverageDirectory', coverageDir], {
      RAW_COVERAGE_DIR: rawDir(instrumenter),
    });
  } finally {
    server.kill();
  }

  const file = resolve(coverageDir, 'coverage-storybook.json');
  if (!existsSync(file)) fail(`${file} not found`);
  return { raw: readRaw(instrumenter), remapped: JSON.parse(readFileSync(file, 'utf8')) };
};

const mapParts = ['statementMap', 'fnMap', 'branchMap'];

// The raw fnMap entries of the two instrumenters have the same keys in a
// different order. Sorted keys make the comparison independent of that order.
const canonical = (value) =>
  JSON.stringify(value, (_, item) =>
    item && typeof item === 'object' && !Array.isArray(item)
      ? Object.fromEntries(Object.entries(item).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)))
      : item,
  );
const hitParts = ['s', 'f', 'b'];

// The maps and the hit or no-hit state of each entry must be equal. The hit
// counts are also compared. A difference in a count is reported, but it does
// not fail the run. The test-runner also saves `window.__coverage__` on the
// `beforeunload` event of the page, and that save can run or not run before
// the browser closes. One earlier run showed different counts. The cause of
// that difference is not confirmed.
const hitState = (counts) => JSON.stringify(counts, (_, value) => (typeof value === 'number' ? value > 0 : value));

const compare = (label, reference, candidate, { names = ['istanbul', 'oxc'], counts = true } = {}) => {
  const [a, b] = names.map((name) => name.padEnd(Math.max(...names.map((n) => n.length)) + 1));
  const referenceKeys = Object.keys(reference).sort();
  const candidateKeys = Object.keys(candidate).sort();
  if (JSON.stringify(referenceKeys) !== JSON.stringify(candidateKeys)) {
    fail(`${label}: file keys differ\n  ${a}${referenceKeys.join(', ')}\n  ${b}${candidateKeys.join(', ')}`);
  }
  let countsEqual = true;
  for (const key of referenceKeys) {
    for (const part of mapParts) {
      const left = canonical(reference[key][part]);
      const right = canonical(candidate[key][part]);
      if (left !== right) fail(`${label}: ${part} differs for ${key}\n  ${a}${left}\n  ${b}${right}`);
    }
    for (const part of hitParts) {
      const left = reference[key][part];
      const right = candidate[key][part];
      if (hitState(left) !== hitState(right)) {
        fail(`${label}: covered entries in ${part} differ for ${key}\n  ${a}${JSON.stringify(left)}\n  ${b}${JSON.stringify(right)}`);
      }
      if (counts && JSON.stringify(left) !== JSON.stringify(right)) {
        countsEqual = false;
        console.log(`note: ${label}: hit counts in ${part} differ for ${key}\n  ${a}${JSON.stringify(left)}\n  ${b}${JSON.stringify(right)}`);
      }
    }
  }
  const countText = counts ? `; hit counts are ${countsEqual ? 'equal' : 'different'}` : '';
  console.log(`${label}: maps and covered entries are equal${countText}.`);
};

const remap = async (raw) => {
  const store = libSourceMaps.createSourceMapStore();
  const map = await store.transformCoverage(libCoverage.createCoverageMap(raw));
  return map.toJSON();
};

const summary = (coverage) =>
  Object.entries(coverage).map(([key, data]) => {
    const totals = libCoverage.createFileCoverage(data).toSummary().toJSON();
    const pct = (part) => `${totals[part].covered}/${totals[part].total}`;
    return `${key.slice(root.length + 1)}: statements ${pct('statements')}, branches ${pct('branches')}, functions ${pct('functions')}, lines ${pct('lines')}`;
  });

// The raw check is only real when the raw entries are not remapped yet.
const requireInputSourceMaps = (label, coverage) => {
  for (const [key, data] of Object.entries(coverage)) {
    if (!data.inputSourceMap) fail(`${label}: ${key} has no inputSourceMap, so it is not raw coverage`);
  }
};

const runs = {};
for (const [index, instrumenter] of instrumenters.entries()) {
  runs[instrumenter] = await collect(instrumenter, basePort + index);
}

for (const name of expectedFiles) {
  for (const instrumenter of instrumenters) {
    for (const stage of ['raw', 'remapped']) {
      if (!Object.keys(runs[instrumenter][stage]).some((key) => key.endsWith(name))) {
        fail(`${instrumenter}: no ${stage} coverage entry for ${name}`);
      }
    }
  }
}

console.log('');
for (const instrumenter of instrumenters) requireInputSourceMaps(`${instrumenter} raw coverage`, runs[instrumenter].raw);
compare('raw coverage (before the remap)', runs.istanbul.raw, runs.oxc.raw);
const inputMaps = (coverage) =>
  JSON.stringify(Object.keys(coverage).sort().map((key) => coverage[key].inputSourceMap.mappings));
if (inputMaps(runs.istanbul.raw) !== inputMaps(runs.oxc.raw)) fail('raw coverage: the inputSourceMap mappings differ');
console.log('raw coverage (before the remap): inputSourceMap mappings are equal.');

compare('remapped coverage (nyc merge)', runs.istanbul.remapped, runs.oxc.remapped);

for (const instrumenter of instrumenters) {
  const own = await remap(runs[instrumenter].raw);
  // nyc also merges the `beforeunload` snapshots, which the postVisit hook
  // does not see. Thus the hit counts are higher in the nyc output, and only
  // the maps and the covered state are compared here.
  compare(`${instrumenter}: own remap against nyc merge`, own, runs[instrumenter].remapped, {
    names: ['own remap', 'nyc merge'],
    counts: false,
  });
}

console.log('\nRemapped coverage (equal for both instrumenters):');
for (const line of summary(runs.oxc.remapped)) console.log(`  ${line}`);
console.log('\nOK: istanbul-lib-instrument and oxc-coverage-instrument give the same Storybook coverage.');
