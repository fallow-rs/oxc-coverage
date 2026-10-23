import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import inspector from 'node:inspector';
import { dirname, join } from 'node:path';
import { promisify } from 'node:util';
import vm from 'node:vm';
import { fileURLToPath } from 'node:url';

import { v8ToIstanbul } from '../crates/oxc_coverage_instrument_napi/index.js';

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), '..');
const fixturePath = join(
  repoRoot,
  'crates',
  'oxc_coverage_instrument',
  'tests',
  'conformance',
  'fixtures',
  '24-nested-functions.js',
);
const fixtureFilename = 'crates/oxc_coverage_instrument/tests/conformance/fixtures/24-nested-functions.js';
const inlineFilename = 'inspector-inline.js';
const inlineUrl = 'oxc-coverage-instrument://inline-precise-coverage.js';
const nestedArmFilename = 'inspector-nested-arm.js';
const nestedArmUrl = 'oxc-coverage-instrument://nested-function-arm.js';
const unicodeFilename = 'inspector-unicode.js';
const unicodeUrl = 'oxc-coverage-instrument://unicode-offsets.js';
const lineTerminatorFilename = 'inspector-line-terminators.js';
const lineTerminatorUrl = 'oxc-coverage-instrument://line-terminators.js';
const fixtureUrl = 'oxc-coverage-instrument://repository-nested-functions.js';
const privateTwinFilename = 'inspector-private-twin.js';
const privateTwinUrl = 'oxc-coverage-instrument://private-twin.js';

const withSourceUrl = (source, url) => `${source.trimEnd()}\n//# sourceURL=${url}\n`;

const captureFunctions = async (source, url, execute) => {
  const session = new inspector.Session();
  const post = promisify(session.post).bind(session);
  const context = vm.createContext({});

  session.connect();
  try {
    await post('Profiler.enable');
    await post('Profiler.startPreciseCoverage', { callCount: true, detailed: true });
    vm.runInContext(source, context);
    execute(context);

    const { result } = await post('Profiler.takePreciseCoverage');
    const scripts = result.filter((entry) => entry.url === url);
    assert.equal(scripts.length, 1, `precise coverage must contain exactly one script URL ${url}`);
    const [script] = scripts;
    assert(script.functions.length > 0, `precise coverage must contain functions for ${url}`);
    return script.functions;
  } finally {
    await post('Profiler.stopPreciseCoverage').catch(() => undefined);
    await post('Profiler.disable').catch(() => undefined);
    session.disconnect();
  }
};

const assertCounterMetadata = (coverage) => {
  assert.deepEqual(Object.keys(coverage.s).sort(), Object.keys(coverage.statementMap).sort());
  assert.deepEqual(Object.keys(coverage.f).sort(), Object.keys(coverage.fnMap).sort());
  assert.deepEqual(Object.keys(coverage.b).sort(), Object.keys(coverage.branchMap).sort());
  for (const [id, metadata] of Object.entries(coverage.branchMap)) {
    assert.equal(
      coverage.b[id].length,
      metadata.locations.length,
      `branch ${id} counter vector must align with its locations`,
    );
  }
};

const functionCount = (coverage, name) => {
  const entry = Object.entries(coverage.fnMap).find(([, metadata]) => metadata.name === name);
  assert(entry, `Istanbul function metadata must contain ${name}`);
  return coverage.f[entry[0]];
};

const ifBranchCounts = (coverage) => {
  const branch = Object.entries(coverage.branchMap).find(([, metadata]) => metadata.type === 'if');
  assert(branch, 'Istanbul branch metadata must contain an if statement');
  return coverage.b[branch[0]];
};

const runInlineSmoke = async () => {
  const source = withSourceUrl(
    `globalThis.topLevel = 1;
function repeated(value) {
  function nested(input) {
    return input * 2;
  }
  if (value > 0) {
    return nested(value);
  } else {
    return -1;
  }
}
repeated(1);
repeated(2);`,
    inlineUrl,
  );
  const functions = await captureFunctions(source, inlineUrl, () => undefined);
  const coverage = JSON.parse(v8ToIstanbul(source, inlineFilename, JSON.stringify(functions)));

  assert.equal(coverage.path, inlineFilename);
  assertCounterMetadata(coverage);
  assert(Object.values(coverage.s).some((count) => count > 0), 'an executed statement must be hit');
  assert.equal(functionCount(coverage, 'repeated'), 2);
  assert.equal(functionCount(coverage, 'nested'), 2);

  assert.deepEqual(ifBranchCounts(coverage), [2, 0], 'taken and untaken if arms must remain distinct');
};

const runNestedFunctionArmSmoke = async () => {
  const source = withSourceUrl(
    `if (true) { function nestedArm() {} }
nestedArm();
nestedArm();`,
    nestedArmUrl,
  );
  const functions = await captureFunctions(source, nestedArmUrl, () => undefined);
  const coverage = JSON.parse(v8ToIstanbul(source, nestedArmFilename, JSON.stringify(functions)));

  assert.equal(coverage.path, nestedArmFilename);
  assertCounterMetadata(coverage);
  assert.equal(functionCount(coverage, 'nestedArm'), 2);
  assert.deepEqual(
    ifBranchCounts(coverage),
    [1, 0],
    'nested function calls must not replace the enclosing if count',
  );
};

const runUnicodeOffsetSmoke = async () => {
  const source = withSourceUrl(
    `const prefix = '😀😀😀😀😀😀é';
function unicodeBranch(value) {
  if (value) {
    return '✓';
  } else {
    return 'miss';
  }
}
unicodeBranch(true);
unicodeBranch(true);`,
    unicodeUrl,
  );
  const functions = await captureFunctions(source, unicodeUrl, () => undefined);
  const coverage = JSON.parse(v8ToIstanbul(source, unicodeFilename, JSON.stringify(functions)));

  assert.equal(coverage.path, unicodeFilename);
  assertCounterMetadata(coverage);
  assert.equal(functionCount(coverage, 'unicodeBranch'), 2);
  assert.deepEqual(
    ifBranchCounts(coverage),
    [2, 0],
    'UTF-16 inspector offsets must preserve taken and untaken arms',
  );
};

const runLineTerminatorSmoke = async () => {
  const source = withSourceUrl(
    'globalThis.first = 1;\r' +
      'globalThis.second = 2;\r\n' +
      'globalThis.third = 3;\u2028' +
      'globalThis.fourth = 4;\u2029' +
      'globalThis.fifth = 5;',
    lineTerminatorUrl,
  );
  const functions = await captureFunctions(source, lineTerminatorUrl, () => undefined);
  const coverage = JSON.parse(
    v8ToIstanbul(source, lineTerminatorFilename, JSON.stringify(functions)),
  );
  const lines = Object.values(coverage.statementMap).map(({ start }) => start.line);

  assert.equal(coverage.path, lineTerminatorFilename);
  assertCounterMetadata(coverage);
  assert.deepEqual(lines.slice(0, 5), [1, 2, 3, 4, 5]);
  assert(Object.values(coverage.s).slice(0, 5).every((count) => count > 0));
};

// fallow-rs/oxc-coverage-instrument#303: a private method and a public method
// with the same body. Both must get a function entry with the V8 call count.
const runPrivateMethodSmoke = async () => {
  const source = withSourceUrl(
    `class Twin {
  #privateMethod(value) {
    if (value === 0) return 10;
    if (value === 1) return 20;
    return 70;
  }
  publicMethod(value) {
    if (value === 0) return 10;
    if (value === 1) return 20;
    return 70;
  }
  callPrivate(value) {
    return this.#privateMethod(value);
  }
}
const twin = new Twin();
for (let value = 0; value <= 2; value++) {
  twin.callPrivate(value);
  twin.publicMethod(value);
}`,
    privateTwinUrl,
  );
  const functions = await captureFunctions(source, privateTwinUrl, () => undefined);
  assert(
    functions.some(({ functionName }) => functionName === '#privateMethod'),
    'V8 must report the private method as its own function',
  );
  const coverage = JSON.parse(v8ToIstanbul(source, privateTwinFilename, JSON.stringify(functions)));

  assertCounterMetadata(coverage);
  assert.equal(functionCount(coverage, '#privateMethod'), 3);
  assert.equal(functionCount(coverage, 'publicMethod'), 3);
  const entry = (name) => Object.values(coverage.fnMap).find((metadata) => metadata.name === name);
  assert.deepEqual(entry('#privateMethod').decl, {
    start: { line: 2, column: 2 },
    end: { line: 2, column: 16 },
  });
  assert.deepEqual(entry('publicMethod').decl, {
    start: { line: 7, column: 2 },
    end: { line: 7, column: 14 },
  });
  assert(Object.values(coverage.s).every((count) => count > 0), 'every statement must be hit');
};

const runRepositoryFixtureSmoke = async () => {
  const fixtureSource = await readFile(fixturePath, 'utf8');
  const source = withSourceUrl(fixtureSource, fixtureUrl);
  const functions = await captureFunctions(source, fixtureUrl, (context) => {
    vm.runInContext('outer(4)', context);
  });
  const coverage = JSON.parse(v8ToIstanbul(source, fixtureFilename, JSON.stringify(functions)));

  assert.equal(coverage.path, fixtureFilename);
  assertCounterMetadata(coverage);
  assert(Object.values(coverage.s).every((count) => count > 0), 'fixture statements must be hit');
  assert.equal(functionCount(coverage, 'outer'), 1);
  assert.equal(functionCount(coverage, 'inner'), 1);
};

await runInlineSmoke();
await runNestedFunctionArmSmoke();
await runUnicodeOffsetSmoke();
await runLineTerminatorSmoke();
await runPrivateMethodSmoke();
await runRepositoryFixtureSmoke();
console.log('PASS v8 inspector coverage converts to stable Istanbul behavior');
