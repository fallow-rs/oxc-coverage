# Jest and nyc with the Babel plugin

This example uses `oxc-coverage-instrument/babel` in place of
`babel-plugin-istanbul`, in two test setups:

- Jest with `coverageProvider: 'babel'` and `babel-jest`.
- nyc with mocha and `@babel/register`.

The sources are TypeScript and JavaScript (`src/`). One file, `src/report.ts`,
has no test. Jest `collectCoverageFrom` and nyc `all` still report it.

## Run it

Build the native binding first (see the repository `AGENTS.md`), then:

```bash
npm install
npm run coverage            # Jest, oxc-coverage-instrument/babel
npm run coverage:istanbul   # Jest, babel-plugin-istanbul
npm run nyc                 # nyc + mocha, oxc-coverage-instrument/babel
npm run nyc:istanbul        # nyc + mocha, babel-plugin-istanbul
npm run verify
```

`npm run verify` compares each pair of `coverage-final.json` files:

- Jest: the two files must be equal byte for byte.
- nyc: the coverage data must be equal. nyc also writes `contentHash`, a hash
  of the instrumented code. The two plugins emit different code, so the check
  ignores that field.

The script also confirms that, in the Jest setup, this plugin instruments the
files and `babel-plugin-istanbul` skips them.

## The configuration

`babel.config.js` selects the plugin. `COVERAGE_PLUGIN=istanbul` selects
`babel-plugin-istanbul`, for the comparison runs.

For Jest, the plugin has `onlyWithIstanbul: true`. `babel-jest` adds
`babel-plugin-istanbul` only for `jest --coverage`, so a test run without
coverage does not instrument. With coverage, this plugin takes the options of
`babel-plugin-istanbul`, and `babel-plugin-istanbul` skips the file.

For nyc, the plugin has no options. It reads `include`, `exclude`, and
`extension` from the `NYC_CONFIG` environment variable that nyc sets. The nyc
config in `package.json` sets `"instrument": false` and `"sourceMap": false`,
because Babel instruments the files.
