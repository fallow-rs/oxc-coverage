// karma-coverage instrumenter adapter for oxc-coverage-instrument.
//
// karma-coverage accepts custom instrumenters in
// `coverageReporter.instrumenters`. For each module value it first looks for
// an `Instrumenter` constructor, then calls `new Instrumenter(options)` once
// per file. The instance must expose `instrument(code, filename, callback,
// inputSourceMap)` and `lastSourceMap()`. With `includeAllSources: true` it
// also reads `lastFileCoverage()`.
//
// Usage in karma.conf.js:
//
//   module.exports = (config) => {
//     config.set({
//       preprocessors: { 'src/**/*.js': ['coverage'] },
//       reporters: ['progress', 'coverage'],
//       coverageReporter: {
//         instrumenters: { oxc: require('oxc-coverage-instrument/karma') },
//         // karma-coverage matches these patterns against the absolute path
//         // of each file. Start each pattern with `**/`. A pattern that does
//         // not match selects the istanbul instrumenter without a warning.
//         instrumenter: { '**/*.js': 'oxc' },
//         instrumenterOptions: { oxc: { compat: 'istanbul' } },
//       },
//     });
//   };

const { createOxcInstrumenter } = require('./vitest.js');

// Options that the oxc-coverage-instrument core understands. karma-coverage
// also sends `autoWrap`, `codeGenerationOptions`, and the other
// istanbul-lib-instrument options. Those control Babel code generation, which
// this instrumenter does not use, so the adapter does not forward them.
const FORWARDED_OPTIONS = [
  'compat',
  'coverageVariable',
  'ignoreClassMethods',
  'reportLogic',
  'trackOptionalChainBranches',
  'functionIdentityOverlay',
  'nameCallbackArguments',
  'stripTypescript',
  'experimentalDecorators',
  'emitDecoratorMetadata',
  'strictNullChecks',
];

const pickForwardedOptions = (options) => {
  const picked = {};
  for (const key of FORWARDED_OPTIONS) {
    if (options[key] !== undefined) picked[key] = options[key];
  }
  return picked;
};

/**
 * Instrumenter with the shape that karma-coverage expects from an entry in
 * `coverageReporter.instrumenters`. It also matches the
 * `istanbul-lib-instrument` Instrumenter interface.
 */
class Instrumenter {
  /**
   * @param {import('./karma.d.ts').OxcKarmaInstrumenterOptions} [options]
   */
  constructor(options) {
    const opts = options || {};
    if (opts.produceSourceMap !== undefined && typeof opts.produceSourceMap !== 'boolean') {
      throw new TypeError(
        `oxc-coverage-instrument/karma: produceSourceMap must be a boolean or undefined, got ${typeof opts.produceSourceMap}`,
      );
    }
    this.produceSourceMap = opts.produceSourceMap === true;
    this.core = createOxcInstrumenter(pickForwardedOptions(opts));
  }

  /**
   * Instruments `code` and returns the instrumented source.
   *
   * @param {string} code
   * @param {string} filename
   * @param {object} [inputSourceMap]
   * @returns {string}
   */
  instrumentSync(code, filename, inputSourceMap) {
    if (typeof code !== 'string') {
      throw new TypeError('oxc-coverage-instrument/karma: code must be a string');
    }
    return this.core.instrumentSync(code, filename || '', inputSourceMap || undefined);
  }

  /**
   * Callback form used by karma-coverage. The callback receives
   * `(error, instrumentedCode)`. The adapter calls the callback outside the
   * error handler, so an error that the callback throws is not reported a
   * second time through the callback.
   *
   * @param {string} code
   * @param {string | ((error: Error | null, code?: string) => void)} filename
   * @param {(error: Error | null, code?: string) => void} [callback]
   * @param {object} [inputSourceMap]
   * @returns {void}
   */
  instrument(code, filename, callback, inputSourceMap) {
    let file = filename;
    let done = callback;
    if (!done && typeof file === 'function') {
      done = file;
      file = null;
    }
    if (typeof done !== 'function') {
      throw new TypeError('oxc-coverage-instrument/karma: instrument() needs a callback');
    }
    let output;
    try {
      output = this.instrumentSync(code, file, inputSourceMap);
    } catch (error) {
      done(error);
      return;
    }
    done(null, output);
  }

  /**
   * Source map of the last instrumented file. Like `istanbul-lib-instrument`,
   * it is `null` unless `produceSourceMap` is true. karma-coverage sets
   * `produceSourceMap` when the file arrives with a source map.
   */
  lastSourceMap() {
    return this.produceSourceMap ? this.core.lastSourceMap() : null;
  }

  /** Coverage object of the last instrumented file. */
  lastFileCoverage() {
    return this.core.lastFileCoverage();
  }
}

/**
 * Creates a karma-coverage instrumenter. Equal to `new Instrumenter(options)`.
 *
 * @param {import('./karma.d.ts').OxcKarmaInstrumenterOptions} [options]
 * @returns {Instrumenter}
 */
const createKarmaInstrumenter = (options) => new Instrumenter(options);

module.exports = { Instrumenter, createKarmaInstrumenter };
