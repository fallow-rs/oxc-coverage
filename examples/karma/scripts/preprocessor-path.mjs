// Runs karma-coverage's own preprocessor in Node with the oxc adapter, for
// the two paths that the browser run does not use: an input file with a
// source map, and `includeAllSources: true`.
import { strict as assert } from 'node:assert';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { resolve } from 'node:path';

const require = createRequire(import.meta.url);
const createCoveragePreprocessor = require('karma-coverage/lib/preprocessor');
const globalCoverageMap = require('karma-coverage/lib/coverage-map');
const oxcKarma = require('oxc-coverage-instrument/karma');

const quietLogger = { create: () => ({ debug() {}, info() {}, error: console.error }) };

const constructed = [];
class RecordingInstrumenter extends oxcKarma.Instrumenter {
  constructor(options) {
    super(options);
    constructed.push(options);
  }
}

const preprocess = createCoveragePreprocessor(quietLogger, process.cwd(), ['coverage'], {
  includeAllSources: true,
  instrumenters: { oxc: { Instrumenter: RecordingInstrumenter } },
  instrumenter: { '**/*.js': 'oxc' },
  instrumenterOptions: { oxc: { compat: 'istanbul' } },
});

const path = resolve('src/cart.js');
const content = readFileSync(path, 'utf8');
// A line-by-line identity map, as an earlier preprocessor can send it.
const lineCount = content.split('\n').length;
const sourceMap = {
  version: 3,
  file: 'cart.js',
  sources: ['cart.original.js'],
  sourcesContent: [content],
  names: [],
  mappings: ['AAAA', ...Array(lineCount - 1).fill('AACA')].join(';'),
};
const file = { originalPath: path, path, sourceMap };

const output = await new Promise((done) => preprocess(content, file, done));

assert.equal(typeof output, 'string', 'the preprocessor returns instrumented code');
assert.equal(constructed.length, 1, 'karma-coverage constructs the oxc adapter');
assert.equal(constructed[0].produceSourceMap, true, 'karma-coverage asks for a source map');
assert.equal(constructed[0].compat, 'istanbul', 'instrumenterOptions reach the adapter');
assert.match(
  output,
  /\/\/# sourceMappingURL=data:application\/json;charset=utf-8;base64,/,
  'karma-coverage appends the source map of the adapter',
);
assert.deepEqual(file.sourceMap.sources, ['cart.original.js'], 'the map composes the input map');
const coverage = globalCoverageMap.get()[path];
assert.ok(coverage, 'includeAllSources registers the coverage from lastFileCoverage()');
assert.ok(Object.keys(coverage.statementMap).length > 0, 'the registered coverage has statements');

// karma-coverage matches the instrumenter patterns against the absolute
// path. A relative pattern does not match, so karma-coverage uses istanbul.
const relativeCount = constructed.length;
const preprocessRelative = createCoveragePreprocessor(quietLogger, process.cwd(), ['coverage'], {
  instrumenters: { oxc: { Instrumenter: RecordingInstrumenter } },
  instrumenter: { 'src/**/*.js': 'oxc' },
});
const plainFile = { originalPath: path, path };
await new Promise((done) => preprocessRelative(content, plainFile, done));
assert.equal(constructed.length, relativeCount, 'a relative pattern does not select the oxc adapter');

console.log('PASS: karma-coverage preprocessor runs the oxc adapter with a source map and includeAllSources, and ignores a relative pattern.');
