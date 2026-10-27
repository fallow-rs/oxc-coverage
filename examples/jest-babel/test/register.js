// mocha loads this file first. @babel/register then transforms the sources
// and the tests with babel.config.js.
require('@babel/register')({ extensions: ['.js', '.ts'] });
