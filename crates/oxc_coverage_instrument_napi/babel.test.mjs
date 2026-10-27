// Tests for oxc-coverage-instrument/babel.
//
// Each test runs @babel/core with this plugin and, where it applies, with
// babel-plugin-istanbul on the same input, and compares the results.

import { strict as assert } from 'node:assert';
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';

import { TraceMap, originalPositionFor } from '@jridgewell/trace-mapping';
import { readInitialCoverage } from 'istanbul-lib-instrument';

const require = createRequire(import.meta.url);
const __dirname = dirname(fileURLToPath(import.meta.url));
const babelModule = await import('@babel/core');
const babel = babelModule.default ?? babelModule;
const oxcPlugin = require('./babel.js');
const istanbulPluginPath = require.resolve('babel-plugin-istanbul');
const typescriptPreset = require.resolve('@babel/preset-typescript');
const FIXTURES = join(__dirname, '..', 'oxc_coverage_instrument', 'tests', 'conformance', 'fixtures');

let passed = 0;
const test = (name, fn) => {
  fn();
  passed++;
  console.log(`  [PASS] ${name}`);
};

// A project directory with a real path, so that `cwd` checks see the same
// path as the plugin.
const project = realpathSync(mkdtempSync(join(tmpdir(), 'oxc-babel-')));
mkdirSync(join(project, 'src'), { recursive: true });

/**
 * Runs Babel on `code` and returns the output and the coverage object that
 * the plugin reports through `onCover`.
 */
const transform = (code, { filename = join(project, 'src', 'a.js'), plugins, presets = [], ...rest }) =>
  babel.transformSync(code, {
    filename,
    cwd: project,
    babelrc: false,
    configFile: false,
    sourceMaps: true,
    plugins,
    presets,
    ...rest,
  });

const coverageOf = (pluginEntry, code, { before = [], after = [], ...options } = {}) => {
  let fileCoverage;
  const [plugin, pluginOptions = {}] = pluginEntry;
  const result = transform(code, {
    ...options,
    plugins: [
      ...before,
      [plugin, { cwd: project, ...pluginOptions, onCover: (_file, coverage) => (fileCoverage = coverage) }],
      ...after,
    ],
  });
  return { result, fileCoverage };
};

const oxc = (options) => [oxcPlugin, options];
const istanbul = (options) => [istanbulPluginPath, options];

const SAMPLE = [
  'const pick = (a, b) => a ?? b;',
  'function grade(score) {',
  '  if (score > 90) return "A";',
  '  return score > 50 ? "B" : "C";',
  '}',
  'class Box {',
  '  constructor(v = 1) { this.v = v; }',
  '  get value() { return this.v; }',
  '  skip() { return 0; }',
  '}',
  'module.exports = { pick, grade, Box };',
  '',
].join('\n');

console.log('Testing oxc-coverage-instrument/babel...\n');

// ---------------------------------------------------------------------------
// Parity with babel-plugin-istanbul
// ---------------------------------------------------------------------------

test('coverage object is equal to babel-plugin-istanbul on every conformance fixture', () => {
  const fixtures = readdirSync(FIXTURES).filter((name) => /\.(js|ts)$/.test(name));
  assert.ok(fixtures.length > 20, 'expected the conformance fixtures');
  for (const name of fixtures) {
    const filename = join(FIXTURES, name);
    const code = readFileSync(filename, 'utf8');
    const options = {
      filename,
      presets: name.endsWith('.ts') ? [typescriptPreset] : [],
    };
    const ours = coverageOf(oxc({ cwd: FIXTURES, exclude: [] }), code, options);
    const theirs = coverageOf(istanbul({ cwd: FIXTURES, exclude: [] }), code, options);
    assert.ok(theirs.fileCoverage, `${name}: babel-plugin-istanbul gave no coverage`);
    assert.equal(
      JSON.stringify(ours.fileCoverage),
      JSON.stringify(theirs.fileCoverage),
      `${name}: coverage object differs`,
    );
  }
});

test('runtime coverage is equal to babel-plugin-istanbul', () => {
  const runCoverage = (pluginEntry) => {
    const { result } = coverageOf(pluginEntry, SAMPLE);
    const context = { module: { exports: {} } };
    vm.runInNewContext(`${result.code}\nmodule.exports.grade(95); module.exports.grade(10); new module.exports.Box().value;`, context);
    return JSON.stringify(Object.values(context.__coverage__)[0]);
  };
  assert.equal(runCoverage(oxc({})), runCoverage(istanbul({})));
});

test('readInitialCoverage() reads the output, as for babel-plugin-istanbul', () => {
  const ours = readInitialCoverage(coverageOf(oxc({}), SAMPLE).result.code);
  const theirs = readInitialCoverage(coverageOf(istanbul({}), SAMPLE).result.code);
  assert.ok(ours, 'Jest and nyc read untested files with readInitialCoverage()');
  assert.deepEqual(ours, theirs);
});

test('output source map points at the original positions', () => {
  const code = ['function first() {', '  return 1;', '}', '', 'function second(n) {', '  return n > 0 ? first() : 0;', '}', ''].join('\n');
  const { result } = coverageOf(oxc({}), code);
  const map = new TraceMap(result.map);
  const lines = result.code.split('\n');
  const find = (text) => {
    const line = lines.findIndex((l) => l.includes(text));
    return originalPositionFor(map, { line: line + 1, column: lines[line].indexOf(text) });
  };
  assert.deepEqual(
    [find('function second').line, find('function second').column],
    [5, 0],
  );
  // The call in the conditional, not the declaration.
  const call = find('first())');
  assert.deepEqual([call.line, call.column], [6, 17]);
  // Babel maps a closing brace through the end location of the block.
  const close = lines.length - 1 - [...lines].reverse().findIndex((l) => l === '}');
  const brace = originalPositionFor(map, { line: close + 1, column: 0 });
  assert.deepEqual([brace.line, brace.column], [7, 0]);
});

test('TypeScript through @babel/preset-typescript: coverage of the original .ts source', () => {
  const code = 'export const len = (s: string | null): number => s?.length ?? 0;\n';
  const options = { filename: join(project, 'src', 'a.ts'), presets: [typescriptPreset] };
  const ours = coverageOf(oxc({}), code, options);
  const theirs = coverageOf(istanbul({}), code, options);
  assert.equal(JSON.stringify(ours.fileCoverage), JSON.stringify(theirs.fileCoverage));
  assert.doesNotMatch(ours.result.code, /: string/);
});

// ---------------------------------------------------------------------------
// Option surface
// ---------------------------------------------------------------------------

test('cwd: a file outside cwd is not instrumented', () => {
  const { result, fileCoverage } = coverageOf(oxc({ cwd: join(project, 'other') }), SAMPLE);
  assert.equal(fileCoverage, undefined);
  assert.doesNotMatch(result.code, /cov_/);
});

test('include and exclude select the files', () => {
  assert.ok(coverageOf(oxc({ include: ['src/**'] }), SAMPLE).fileCoverage);
  assert.equal(coverageOf(oxc({ include: ['lib/**'] }), SAMPLE).fileCoverage, undefined);
  assert.equal(coverageOf(oxc({ exclude: ['src/a.js'] }), SAMPLE).fileCoverage, undefined);
});

test('the default exclude list skips test files', () => {
  const filename = join(project, 'src', 'a.test.js');
  assert.equal(coverageOf(oxc({}), SAMPLE, { filename }).fileCoverage, undefined);
  assert.equal(coverageOf(istanbul({}), SAMPLE, { filename }).fileCoverage, undefined);
});

test('extension filters the files; false instruments every extension', () => {
  const filename = join(project, 'src', 'a.es6');
  assert.equal(coverageOf(oxc({}), SAMPLE, { filename }).fileCoverage, undefined);
  assert.ok(coverageOf(oxc({ extension: false }), SAMPLE, { filename }).fileCoverage);
  assert.ok(coverageOf(oxc({ extension: ['.es6'] }), SAMPLE, { filename }).fileCoverage);
});

test('node_modules is excluded unless excludeNodeModules is false', () => {
  const filename = join(project, 'node_modules', 'pkg', 'index.js');
  assert.equal(coverageOf(oxc({}), SAMPLE, { filename }).fileCoverage, undefined);
  assert.ok(coverageOf(oxc({ excludeNodeModules: false }), SAMPLE, { filename }).fileCoverage);
});

test('coverageVariable sets the global coverage object', () => {
  const { result } = coverageOf(oxc({ coverageVariable: '__oxc_babel__' }), SAMPLE);
  const context = { module: { exports: {} } };
  vm.runInNewContext(result.code, context);
  assert.ok(context.__oxc_babel__);
  assert.equal(context.__coverage__, undefined);
});

test('ignoreClassMethods removes the methods from fnMap, as babel-plugin-istanbul does', () => {
  const ours = coverageOf(oxc({ ignoreClassMethods: ['skip'] }), SAMPLE).fileCoverage;
  const theirs = coverageOf(istanbul({ ignoreClassMethods: ['skip'] }), SAMPLE).fileCoverage;
  assert.ok(!Object.values(ours.fnMap).some((fn) => fn.name === 'skip'));
  assert.equal(JSON.stringify(ours), JSON.stringify(theirs));
});

test('onCover gets the real path; result.metadata.coverage holds the coverage object', () => {
  const calls = [];
  const result = transform(SAMPLE, {
    plugins: [[oxcPlugin, { cwd: project, onCover: (file, coverage) => calls.push([file, coverage]) }]],
  });
  assert.equal(calls.length, 1);
  assert.equal(calls[0][0], join(project, 'src', 'a.js'));
  assert.equal(result.metadata.coverage, calls[0][1]);
});

test('an inline source map becomes coverage.inputSourceMap; useInlineSourceMaps: false turns it off', () => {
  const first = transform('export const f = (x) => x + 1;\n', {
    filename: join(project, 'src', 'b.js'),
    sourceMaps: 'inline',
    plugins: [],
  });
  const ours = coverageOf(oxc({}), first.code, { filename: join(project, 'src', 'b.js') });
  const theirs = coverageOf(istanbul({}), first.code, { filename: join(project, 'src', 'b.js') });
  assert.ok(ours.fileCoverage.inputSourceMap, 'expected inputSourceMap');
  assert.equal(JSON.stringify(ours.fileCoverage), JSON.stringify(theirs.fileCoverage));
  assert.deepEqual(readInitialCoverage(ours.result.code), readInitialCoverage(theirs.result.code));
  const off = coverageOf(oxc({ useInlineSourceMaps: false }), first.code, { filename: join(project, 'src', 'b.js') });
  assert.equal(off.fileCoverage.inputSourceMap, undefined);
});

test('the inputSourceMap option has priority over an inline map', () => {
  const map = { version: 3, sources: ['orig.js'], names: [], mappings: 'AAAA', file: 'a.js' };
  const ours = coverageOf(oxc({ inputSourceMap: map }), SAMPLE).fileCoverage;
  const theirs = coverageOf(istanbul({ inputSourceMap: map }), SAMPLE).fileCoverage;
  assert.deepEqual(ours.inputSourceMap, map);
  assert.equal(JSON.stringify(ours), JSON.stringify(theirs));
});

test('compat: null turns on the oxc-coverage-instrument branch extensions', () => {
  const code = 'let a = null;\na ??= 1;\nmodule.exports = a;\n';
  const strict = coverageOf(oxc({}), code).fileCoverage;
  const extended = coverageOf(oxc({ compat: null }), code).fileCoverage;
  assert.equal(Object.keys(strict.branchMap).length, 0);
  assert.equal(Object.keys(extended.branchMap).length, 1);
});

test('istanbul ignore file: the file is not instrumented', () => {
  const { result } = coverageOf(oxc({}), `/* istanbul ignore file */\n${SAMPLE}`);
  assert.doesNotMatch(result.code, /cov_/);
});

test('invalid options throw a TypeError when Babel loads the plugin', () => {
  for (const options of [
    { compat: 'strict' },
    { coverageVariable: 1 },
    { ignoreClassMethods: [1] },
    { onCover: 'x' },
    { onlyWithIstanbul: 'yes' },
    { coverageGlobalScope: 'window' },
  ]) {
    assert.throws(() => transform(SAMPLE, { plugins: [[oxcPlugin, options]] }), TypeError, JSON.stringify(options));
  }
});

test('NYC_CONFIG is read when the Babel config sets no options', () => {
  const previous = process.env.NYC_CONFIG;
  process.env.NYC_CONFIG = JSON.stringify({ cwd: project, include: ['lib/**'], exclude: [] });
  try {
    const result = transform(SAMPLE, { plugins: [oxcPlugin] });
    assert.doesNotMatch(result.code, /cov_/);
    process.env.NYC_CONFIG = JSON.stringify({ cwd: project, include: ['src/**'], exclude: [] });
    // A new options object, so that Babel does not reuse the cached plugin.
    const second = transform(SAMPLE, { plugins: [[oxcPlugin, {}]] });
    assert.match(second.code, /cov_/);
  } finally {
    if (previous === undefined) delete process.env.NYC_CONFIG;
    else process.env.NYC_CONFIG = previous;
  }
});

test('without test-exclude, the built-in matcher gives the same decisions', () => {
  const blocker = join(project, 'block-test-exclude.cjs');
  writeFileSync(
    blocker,
    `const Module = require('module');
const original = Module._resolveFilename;
Module._resolveFilename = function (request, ...rest) {
  if (request === 'test-exclude') throw Object.assign(new Error('blocked'), { code: 'MODULE_NOT_FOUND' });
  return original.call(this, request, ...rest);
};`,
  );
  const script = `
const babel = require(${JSON.stringify(require.resolve('@babel/core'))});
const plugin = require(${JSON.stringify(join(__dirname, 'babel.js'))});
const decide = (file, options) => {
  let covered = false;
  babel.transformSync('const a = 1;', { filename: file, cwd: ${JSON.stringify(project)}, babelrc: false, configFile: false,
    plugins: [[plugin, { cwd: ${JSON.stringify(project)}, ...options, onCover: () => { covered = true; } }]] });
  return covered;
};
const p = (f) => ${JSON.stringify(project)} + '/' + f;
console.log(JSON.stringify([
  decide(p('src/a.js'), {}),
  decide(p('src/a.test.js'), {}),
  decide(p('src/__tests__/a.js'), {}),
  decide(p('src/a.js'), { include: ['lib/**'] }),
  decide(p('src/a.js'), { exclude: ['src/**', '!src/a.js'] }),
  decide(p('node_modules/x/a.js'), {}),
  decide(p('src/a.es6'), {}),
]));`;
  const run = spawnSync(process.execPath, ['-r', blocker, '-e', script], { encoding: 'utf8' });
  assert.equal(run.status, 0, run.stderr);
  assert.deepEqual(JSON.parse(run.stdout), [true, false, false, false, true, false, false]);
});

// ---------------------------------------------------------------------------
// Plugin order and onlyWithIstanbul
// ---------------------------------------------------------------------------

test('babel-plugin-istanbul in the same run skips the file, before or after this plugin', () => {
  for (const order of ['before', 'after']) {
    const { result, fileCoverage } = coverageOf(oxc({}), SAMPLE, { [order]: [[istanbulPluginPath, { cwd: project }]] });
    assert.ok(fileCoverage);
    assert.match(result.code, /\+\+cov_[0-9a-z]+\.s\[0\]/, `${order}: expected the counters of this plugin`);
    assert.doesNotMatch(result.code, /cov_[0-9a-z]+\(\)\.s\[0\]\+\+/, `${order}: babel-plugin-istanbul also instrumented`);
  }
});

test('onlyWithIstanbul: no instrumentation without babel-plugin-istanbul', () => {
  const { result, fileCoverage } = coverageOf(oxc({ onlyWithIstanbul: true }), SAMPLE);
  assert.equal(fileCoverage, undefined);
  assert.doesNotMatch(result.code, /cov_/);
});

test('onlyWithIstanbul: takes the options of babel-plugin-istanbul and replaces it', () => {
  // babel-jest adds the plugin by its resolved path, after the configured plugins.
  let ours;
  const result = transform(SAMPLE, {
    plugins: [
      [oxcPlugin, { onlyWithIstanbul: true, onCover: (_f, c) => (ours = c) }],
      [istanbulPluginPath, { cwd: project, exclude: [], extension: ['.js'] }],
    ],
  });
  const theirs = coverageOf(istanbul({ exclude: [], extension: ['.js'] }), SAMPLE).fileCoverage;
  assert.equal(JSON.stringify(ours), JSON.stringify(theirs));
  assert.match(result.code, /\+\+cov_[0-9a-z]+\.s\[0\]/);
  const excluded = transform(SAMPLE, {
    plugins: [
      [oxcPlugin, { onlyWithIstanbul: true }],
      [istanbulPluginPath, { cwd: join(project, 'other') }],
    ],
  });
  assert.doesNotMatch(excluded.code, /\+\+cov_/, 'the cwd of babel-plugin-istanbul applies');
});

test('transformFromAst: the plugin instruments in Program.enter', () => {
  const filename = join(project, 'src', 'a.js');
  const ast = babel.parseSync(SAMPLE, { filename, babelrc: false, configFile: false });
  let fileCoverage;
  const result = babel.transformFromAstSync(ast, SAMPLE, {
    filename,
    cwd: project,
    babelrc: false,
    configFile: false,
    plugins: [[oxcPlugin, { cwd: project, onCover: (_f, c) => (fileCoverage = c) }]],
  });
  assert.equal(JSON.stringify(fileCoverage), JSON.stringify(coverageOf(istanbul({}), SAMPLE).fileCoverage));
  assert.match(result.code, /\+\+cov_/);
});

test('the package exports oxc-coverage-instrument/babel', () => {
  // Self-reference through the "exports" field of package.json.
  assert.equal(require.resolve('oxc-coverage-instrument/babel'), join(__dirname, 'babel.js'));
  const pkg = JSON.parse(readFileSync(join(__dirname, 'package.json'), 'utf8'));
  assert.ok(pkg.files.includes('babel.js') && pkg.files.includes('babel.d.ts'));
  assert.equal(pkg.peerDependenciesMeta['@babel/core'].optional, true);
});

console.log(`\nAll ${passed} oxc-coverage-instrument/babel tests passed (Babel ${babel.version}).`);
