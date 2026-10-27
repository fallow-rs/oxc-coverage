// Runs the same tests with one of three instrumenters:
//
//   --instrumenter istanbul    karma-coverage's default (istanbul-lib-instrument)
//   --instrumenter oxc         oxc-coverage-instrument, default profile
//   --instrumenter oxc-strict  oxc-coverage-instrument with compat: 'istanbul'
//
// Each run writes coverage/<instrumenter>/coverage-final.json.
// Set CHROME_BIN when Chrome is not in its usual location.

const INSTRUMENTERS = ['istanbul', 'oxc', 'oxc-strict'];

const readInstrumenter = (argv) => {
  const index = argv.indexOf('--instrumenter');
  const name = index === -1 ? 'oxc' : argv[index + 1];
  if (!INSTRUMENTERS.includes(name)) {
    throw new Error(`--instrumenter must be one of ${INSTRUMENTERS.join(', ')}, got ${name}`);
  }
  return name;
};

module.exports = (config) => {
  const name = readInstrumenter(process.argv);

  config.set({
    frameworks: ['jasmine'],
    files: [
      // A real library: lodash 4.17.21, loaded as a browser script.
      'node_modules/lodash/lodash.js',
      { pattern: 'src/**/*.js', type: 'module', included: false },
      { pattern: 'test/**/*.spec.js', type: 'module' },
    ],
    preprocessors: {
      'node_modules/lodash/lodash.js': ['coverage'],
      'src/**/*.js': ['coverage'],
    },
    reporters: ['progress', 'coverage'],
    coverageReporter: {
      dir: 'coverage',
      subdir: name,
      reporters: [{ type: 'json' }, { type: 'text-summary' }],
      instrumenters: { oxc: require('oxc-coverage-instrument/karma') },
      // karma-coverage matches these patterns against the absolute file
      // path, so they start with `**/`.
      instrumenter: name === 'istanbul' ? {} : { '**/*.js': 'oxc' },
      instrumenterOptions: {
        istanbul: { esModules: true },
        oxc: name === 'oxc-strict' ? { compat: 'istanbul' } : {},
      },
    },
    // Ubuntu runners block the Chrome sandbox, so CI starts Chrome without it.
    browsers: [process.env.CI ? 'ChromeHeadlessNoSandbox' : 'ChromeHeadless'],
    customLaunchers: {
      ChromeHeadlessNoSandbox: { base: 'ChromeHeadless', flags: ['--no-sandbox'] },
    },
    // A cold Chrome start on a busy CI machine can take more than the
    // default 60 seconds.
    captureTimeout: 180000,
    singleRun: true,
  });
};
