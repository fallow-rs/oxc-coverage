// COVERAGE_PLUGIN=istanbul selects babel-plugin-istanbul, for the comparison
// runs. The default is oxc-coverage-instrument/babel.
module.exports = (api) => {
  const useIstanbul = api.cache.using(() => process.env.COVERAGE_PLUGIN === 'istanbul');
  const underNyc = api.cache.using(() => Boolean(process.env.NYC_CONFIG));

  const plugins = [];
  if (underNyc) {
    // nyc runs mocha with @babel/register. The plugin reads the nyc options
    // (include, exclude, extension) from the NYC_CONFIG environment variable.
    plugins.push(useIstanbul ? 'babel-plugin-istanbul' : 'oxc-coverage-instrument/babel');
  } else if (!useIstanbul) {
    // babel-jest adds babel-plugin-istanbul only for `jest --coverage`. With
    // `onlyWithIstanbul`, this plugin runs only then, takes the options of
    // babel-plugin-istanbul, and babel-plugin-istanbul skips the file.
    plugins.push(['oxc-coverage-instrument/babel', { onlyWithIstanbul: true }]);
  }

  return {
    presets: [
      ['@babel/preset-env', { targets: { node: 'current' } }],
      '@babel/preset-typescript',
    ],
    plugins,
  };
};
