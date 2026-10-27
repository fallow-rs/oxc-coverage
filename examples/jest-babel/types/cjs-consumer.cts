// A CommonJS consumer of the types of oxc-coverage-instrument/babel.
import plugin = require('oxc-coverage-instrument/babel');

const options: plugin.OxcBabelPluginOptions = { onlyWithIstanbul: true };

export const fromModule: { name: string } = plugin({}, options);
export const fromDefault: { name: string } = plugin.default({}, options);
