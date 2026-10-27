// Compares the coverage of oxc-coverage-instrument/babel with the coverage of
// babel-plugin-istanbul, for Jest and for nyc with mocha.
//
// Run `npm run coverage`, `npm run coverage:istanbul`, `npm run nyc` and
// `npm run nyc:istanbul` first. The script compares each pair of
// coverage-final.json files that exists, and fails when no pair exists.

import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const require = createRequire(import.meta.url);

const PAIRS = [
  ['jest', 'coverage/coverage-final.json', 'coverage-istanbul/coverage-final.json'],
  ['nyc + mocha', 'coverage-nyc/coverage-final.json', 'coverage-nyc-istanbul/coverage-final.json'],
];

// nyc adds `contentHash`, a hash of the instrumented code. The two plugins
// emit different code, so that field always differs. Coverage data does not
// depend on it.
const IGNORED_FIELDS = new Set(['contentHash']);

const normalize = (coverage) =>
  JSON.parse(
    JSON.stringify(coverage, (key, value) => (IGNORED_FIELDS.has(key) ? undefined : value)),
  );

const readCoverage = (file) => normalize(JSON.parse(readFileSync(join(root, file), 'utf8')));

/** Confirms that the Jest setup runs this plugin, not babel-plugin-istanbul. */
const assertPluginRuns = () => {
  const babel = require('@babel/core');
  const filename = join(root, 'src/cart.ts');
  const { code } = babel.transformSync(readFileSync(filename, 'utf8'), {
    filename,
    cwd: root,
    // babel-jest appends babel-plugin-istanbul in the same way.
    plugins: [[require.resolve('babel-plugin-istanbul'), { cwd: root, exclude: [], extension: ['.ts'] }]],
  });
  assert.match(code, /\+\+cov_[0-9a-z]+\.s\[0\]/, 'expected the counters of oxc-coverage-instrument');
  assert.doesNotMatch(code, /cov_[0-9a-z]+\(\)\.s\[0\]\+\+/, 'babel-plugin-istanbul also instrumented the file');
  console.log('[PASS] babel-jest setup: oxc-coverage-instrument/babel instruments, babel-plugin-istanbul skips');
};

let compared = 0;
for (const [label, oxcFile, istanbulFile] of PAIRS) {
  if (!existsSync(join(root, oxcFile)) || !existsSync(join(root, istanbulFile))) {
    console.log(`[SKIP] ${label}: run both coverage commands first`);
    continue;
  }
  const oxc = readCoverage(oxcFile);
  const istanbul = readCoverage(istanbulFile);
  assert.deepEqual(Object.keys(oxc).sort(), Object.keys(istanbul).sort(), `${label}: file lists differ`);
  for (const file of Object.keys(istanbul)) {
    assert.deepEqual(oxc[file], istanbul[file], `${label}: coverage of ${file} differs`);
  }
  if (label === 'jest') {
    // Jest writes the coverage objects as they are, so the files must also be
    // equal byte for byte.
    assert.equal(
      readFileSync(join(root, oxcFile), 'utf8'),
      readFileSync(join(root, istanbulFile), 'utf8'),
      'jest: coverage-final.json is not equal byte for byte',
    );
  }
  console.log(`[PASS] ${label}: coverage-final.json is equal for ${Object.keys(oxc).length} files`);
  compared++;
}
assertPluginRuns();
assert.ok(compared > 0, 'no coverage pair to compare');
