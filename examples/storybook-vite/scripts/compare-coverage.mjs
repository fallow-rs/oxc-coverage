// Runs the Storybook test-runner with coverage two times: one time with the
// default istanbul-lib-instrument and one time with oxc-coverage-instrument.
// Then it compares the two coverage files, before and after the source-map
// remap. It fails when a map or the covered state of an entry differs.
//
// Each run builds a static Storybook, serves it with http-server, and runs
// `test-storybook --coverage` against it. The test-runner reads
// `window.__coverage__` after each story and writes
// coverage/<instrumenter>/coverage-storybook.json.

import { spawn, spawnSync } from 'node:child_process';
import { existsSync, readFileSync, rmSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import libCoverage from 'istanbul-lib-coverage';
import libSourceMaps from 'istanbul-lib-source-maps';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
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
    run(bin('test-storybook'), ['--url', url, '--coverage', '--coverageDirectory', coverageDir]);
  } finally {
    server.kill();
  }

  const file = resolve(coverageDir, 'coverage-storybook.json');
  if (!existsSync(file)) fail(`${file} not found`);
  return JSON.parse(readFileSync(file, 'utf8'));
};

const mapParts = ['statementMap', 'fnMap', 'branchMap'];
const hitParts = ['s', 'f', 'b'];

// A hit count is the number of times a story ran the code before the
// test-runner read the counters. React can render a story one more time
// before that read, so the counts can change from run to run with the same
// instrumenter. The maps and the hit or no-hit state of each entry must be
// equal. Different counts are reported, but they do not fail the run.
const hitState = (counts) => JSON.stringify(counts, (_, value) => (typeof value === 'number' ? value > 0 : value));

const compare = (label, reference, candidate) => {
  const referenceKeys = Object.keys(reference).sort();
  const candidateKeys = Object.keys(candidate).sort();
  if (JSON.stringify(referenceKeys) !== JSON.stringify(candidateKeys)) {
    fail(`${label}: file keys differ\n  istanbul: ${referenceKeys.join(', ')}\n  oxc:      ${candidateKeys.join(', ')}`);
  }
  let countsEqual = true;
  for (const key of referenceKeys) {
    for (const part of mapParts) {
      const left = JSON.stringify(reference[key][part]);
      const right = JSON.stringify(candidate[key][part]);
      if (left !== right) fail(`${label}: ${part} differs for ${key}\n  istanbul: ${left}\n  oxc:      ${right}`);
    }
    for (const part of hitParts) {
      const left = reference[key][part];
      const right = candidate[key][part];
      if (hitState(left) !== hitState(right)) {
        fail(`${label}: covered entries in ${part} differ for ${key}\n  istanbul: ${JSON.stringify(left)}\n  oxc:      ${JSON.stringify(right)}`);
      }
      if (JSON.stringify(left) !== JSON.stringify(right)) {
        countsEqual = false;
        console.log(`note: ${label}: hit counts in ${part} differ for ${key} (render count)\n  istanbul: ${JSON.stringify(left)}\n  oxc:      ${JSON.stringify(right)}`);
      }
    }
  }
  console.log(`${label}: maps and covered entries are equal; hit counts are ${countsEqual ? 'equal' : 'different'}.`);
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

const raw = {};
for (const [index, instrumenter] of instrumenters.entries()) {
  raw[instrumenter] = await collect(instrumenter, basePort + index);
}

for (const name of expectedFiles) {
  if (!Object.keys(raw.oxc).some((key) => key.endsWith(name))) fail(`no coverage entry for ${name}`);
}

compare('raw coverage', raw.istanbul, raw.oxc);
const remapped = { istanbul: await remap(raw.istanbul), oxc: await remap(raw.oxc) };
compare('remapped coverage', remapped.istanbul, remapped.oxc);

console.log('\nRemapped coverage (equal for both instrumenters):');
for (const line of summary(remapped.oxc)) console.log(`  ${line}`);
console.log('\nOK: istanbul-lib-instrument and oxc-coverage-instrument give the same Storybook coverage.');
