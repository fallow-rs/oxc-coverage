/**
 * The Istanbul file coverage object for one file.
 */
export interface BabelFileCoverage {
  path: string;
  statementMap: Record<string, unknown>;
  fnMap: Record<string, unknown>;
  branchMap: Record<string, unknown>;
  s: Record<string, number>;
  f: Record<string, number>;
  b: Record<string, number[]>;
  inputSourceMap?: unknown;
}

/**
 * Options of `oxc-coverage-instrument/babel`.
 *
 * The plugin accepts the options of `babel-plugin-istanbul`. When the Babel
 * config sets no option other than `cwd` and `nycrcPath`, the plugin reads the
 * `NYC_CONFIG` environment variable that nyc sets. The plugin does not read
 * `.nycrc` files.
 */
export interface OxcBabelPluginOptions {
  /**
   * The root for `include` and `exclude`. Files outside `cwd` are not
   * instrumented. Defaults to `NYC_CWD`, then `process.cwd()`.
   */
  cwd?: string;
  /** Glob patterns of the files to instrument, relative to `cwd`. */
  include?: string | string[];
  /**
   * Glob patterns of the files not to instrument, relative to `cwd`. A
   * pattern that starts with `!` includes a file again. Defaults to the
   * `@istanbuljs/schema` list, which excludes test files.
   */
  exclude?: string | string[];
  /**
   * File extensions to instrument. `false` instruments every extension.
   * Defaults to `.js`, `.cjs`, `.mjs`, `.ts`, `.tsx` and `.jsx`.
   */
  extension?: string | string[] | false;
  /** Set to `false` to instrument files in `node_modules`. Defaults to `true`. */
  excludeNodeModules?: boolean;
  /**
   * Use the input source map that Babel found in the file (an inline
   * `sourceMappingURL` comment, or the Babel `inputSourceMap` option). The
   * map is attached to the coverage object as `inputSourceMap`, as
   * babel-plugin-istanbul does. Defaults to `true`.
   */
  useInlineSourceMaps?: boolean;
  /**
   * An input source map to attach to the coverage object. It has priority
   * over the map that Babel found.
   */
  inputSourceMap?: object;
  /** Class method names to exclude from coverage. */
  ignoreClassMethods?: string[];
  /** The global coverage variable. Defaults to `__coverage__`. */
  coverageVariable?: string;
  /** Count truthy values of logical expression operands (`bT`). */
  reportLogic?: boolean;
  /**
   * Only `'this'` is accepted. The coverage object is always on
   * `globalThis`.
   */
  coverageGlobalScope?: 'this';
  /** Ignored. Accepted for compatibility with babel-plugin-istanbul. */
  coverageGlobalScopeFunc?: boolean;
  /** Ignored. Accepted for compatibility with babel-plugin-istanbul. */
  nycrcPath?: string;
  /**
   * Called once for each instrumented file, with the real path and the file
   * coverage object.
   */
  onCover?: (filePath: string, fileCoverage: BabelFileCoverage) => void;
  /**
   * The coverage profile. `'istanbul'` gives the same statement, function and
   * branch entries as babel-plugin-istanbul. `null` turns on the extensions
   * of oxc-coverage-instrument: branches for `??=`, `||=`, `&&=` and optional
   * chains. Defaults to `'istanbul'`.
   */
  compat?: 'istanbul' | null;
  /**
   * Instrument a file only when babel-plugin-istanbul is in the same Babel
   * run, and use the options of that plugin as defaults. babel-plugin-istanbul
   * then finds the counters of this plugin and skips the file. Use this with
   * babel-jest, which adds babel-plugin-istanbul only for `jest --coverage`.
   * The plugin finds babel-plugin-istanbul by its module path or by the name
   * `istanbul` in the Babel plugin list. Defaults to `false`.
   */
  onlyWithIstanbul?: boolean;
}

/** The Babel plugin object that the plugin function returns. */
export interface OxcBabelPlugin {
  name: string;
  visitor: Record<string, unknown>;
}

/**
 * The Babel plugin. Use it in a Babel config as
 * `'oxc-coverage-instrument/babel'`, in place of `'istanbul'` or
 * `'babel-plugin-istanbul'`.
 *
 * After the transform, `result.metadata.coverage` holds the file coverage
 * object of the file.
 */
export declare function babelPluginOxcCoverage(api: object, options?: OxcBabelPluginOptions): OxcBabelPlugin;

export default babelPluginOxcCoverage;
