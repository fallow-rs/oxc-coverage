// An ES module consumer of the types of oxc-coverage-instrument/babel.
import plugin, {
  babelPluginOxcCoverage,
  type BabelFileCoverage,
  type OxcBabelPluginOptions,
} from 'oxc-coverage-instrument/babel';

const options: OxcBabelPluginOptions = {
  exclude: ['test/**'],
  onCover: (_path: string, coverage: BabelFileCoverage) => void coverage.path,
};

export const fromDefault: { name: string } = plugin({}, options);
export const fromNamed: { name: string } = babelPluginOxcCoverage({}, options);
