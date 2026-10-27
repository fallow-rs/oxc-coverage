// Babel plugin for oxc-coverage-instrument. It replaces babel-plugin-istanbul.
//
// Usage in babel.config.js:
//
//   module.exports = {
//     plugins: ['oxc-coverage-instrument/babel'],
//   };
//
// How the plugin works
// --------------------
//
// babel-plugin-istanbul instruments the AST of the original source, so the
// coverage map points at the original source. This plugin also instruments
// the original source, with the native instrumenter, before Babel parses it:
//
//   1. Babel calls `parserOverride(code, parserOpts, parse)` of the plugin.
//      `code` is the source text of the file.
//   2. The plugin calls the synchronous `instrument()` of the native binding.
//      Babel plugins are synchronous, so an async API is not possible.
//   3. The plugin parses the instrumented text with the Babel parser and the
//      parser options of the run, so TypeScript, JSX and other syntax that the
//      presets enable still parse.
//   4. The plugin moves every `loc`, `start` and `end` of that AST from the
//      coordinates of the instrumented text back to the coordinates of the
//      source text. The native source map gives that relation. A node whose
//      start has no exact mapping loses its location (for example, an inserted
//      counter), so it gets no segment in the output source map.
//   5. Babel uses that AST for the file. All plugins and presets then
//      transform the instrumented AST, in their usual order.
//
// Babel composes its output source map with the input source map of the file
// as usual, because the locations are in the coordinates of the source text.
//
// Why `parserOverride`, and not the other approaches
// --------------------------------------------------
//
// * `Program.enter` with a new AST: Babel has then already parsed the file and
//   built the scope. The plugin must parse a second time, replace the program
//   body and crawl the scope again. On a real project that made the plugin
//   slower than babel-plugin-istanbul. The plugin uses this approach only for
//   `onlyWithIstanbul`, because only `Program.enter` can see the other plugins
//   of the run.
// * `Program.exit` with a regenerate step: at exit, the other plugins have
//   already changed the AST (TypeScript removed, modules converted). To
//   instrument there, the plugin must generate code from the current AST,
//   instrument that code, and compose the coverage map back through the
//   generated map. That costs one more generate and parse per file, and the
//   coverage then describes the transpiled code (helpers, lowered loops), not
//   the original source.
//
// The cost of `parserOverride`: Babel permits only one plugin that overrides
// the parser in a run. A second plugin that returns an AST from
// `parserOverride` (for example a Hermes parser plugin) makes Babel stop with
// an error.
//
// Syntax
// ------
//
// The native parser selects the language from the file extension, and Babel
// selects it from its parser plugins. The plugin reads the `jsx` and
// `typescript` parser plugins and `allowReturnOutsideFunction`, and gives the
// same language to the native parser (see `nativeSourceType`). Thus JSX in a
// `.js` file works. These limits stay:
//
// * Flow syntax is not supported, because the native parser does not parse
//   Flow.
// * `allowReturnOutsideFunction` works only without JSX and TypeScript.
// * Syntax from Babel parser plugins for proposals that the native parser
//   does not parse (for example `pipelineOperator`, `doExpressions` and
//   `exportDefaultFrom`) makes the plugin stop with a parse error.
//
// For these files, babel-plugin-istanbul instruments the file and this plugin
// does not. The error message names the file.
//
// Plugin order
// ------------
//
// With `parserOverride`, the position of the plugin in the list has no effect:
// every plugin sees the instrumented AST of the original source.
//
// With `onlyWithIstanbul`, put this plugin first, as for babel-plugin-istanbul.
// The plugin then reads `file.code`, not the current AST, so a change that an
// earlier plugin made in `pre()` or `Program.enter` is lost.
//
// The plugin names its coverage function as babel-plugin-istanbul does. If
// babel-plugin-istanbul runs later in the same Babel run, it finds that
// function and skips the file. babel-jest adds babel-plugin-istanbul after the
// plugins of the Babel config, so `onlyWithIstanbul` replaces it in Jest.

'use strict';

const { createHash } = require('node:crypto');
const { realpathSync } = require('node:fs');
const nodePath = require('node:path');

const { instrument } = require('./index.js');

const PLUGIN_NAME = 'oxc-coverage-instrument';
// The key and value that `istanbul-lib-instrument` writes into the coverage
// object. `readInitialCoverage()` (Jest, nyc `--all`) finds the coverage
// object by this key. The value is the SHA-1 of the name and schema version
// of `istanbul-lib-instrument`, as in its `constants.js`.
const MAGIC_KEY = '_coverageSchema';
const MAGIC_VALUE = createHash('sha1').update('istanbul-lib-instrument@4').digest('hex');
const DEFAULT_COVERAGE_VARIABLE = '__coverage__';
const NATIVE_COVERAGE_FN_RE = /\bvar (cov_[0-9a-z]+) = \(function \(\) \{ var path = /;

// The defaults of `@istanbuljs/schema` for babel-plugin-istanbul.
const DEFAULT_EXTENSIONS = ['.js', '.cjs', '.mjs', '.ts', '.tsx', '.jsx'];
const DEFAULT_EXCLUDE = [
  'coverage/**',
  'packages/*/test{,s}/**',
  '**/*.d.ts',
  'test{,s}/**',
  'test{,-*}.{js,cjs,mjs,ts,tsx,jsx}',
  '**/*{.,-}test.{js,cjs,mjs,ts,tsx,jsx}',
  '**/__tests__/**',
  '**/{ava,babel,nyc}.config.{js,cjs,mjs}',
  '**/jest.config.{js,cjs,mjs,ts}',
  '**/{karma,rollup,webpack}.config.js',
  '**/.{eslint,mocha}rc.{js,cjs}',
];

// Options that babel-plugin-istanbul ignores when it decides whether the Babel
// config sets options explicitly.
const LOCATION_ONLY_OPTIONS = new Set(['cwd', 'nycrcPath']);
// Options that the Babel config can set on this plugin only.
const OWN_OPTIONS = new Set(['compat', 'onlyWithIstanbul']);

/**
 * Returns the real path of a file, or the path itself when it does not exist.
 *
 * @param {string} file
 * @returns {string}
 */
const getRealpath = (file) => {
  try {
    return realpathSync(file) || file;
  } catch {
    return file;
  }
};

/**
 * Returns the name of the coverage function that `istanbul-lib-instrument`
 * makes for a file path.
 *
 * @param {string} filename
 * @returns {string}
 */
const istanbulCoverageFunctionName = (filename) =>
  `cov_${Number.parseInt(createHash('sha1').update(filename).digest('hex').slice(0, 12), 16).toString(36)}`;

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

/**
 * Merges the plugin options with the nyc config, as babel-plugin-istanbul
 * does. Explicit Babel options win. With no explicit options, the plugin uses
 * the `NYC_CONFIG` environment variable that nyc sets.
 *
 * @param {Record<string, unknown>} opts
 * @returns {Record<string, unknown>}
 */
const findConfig = (opts) => {
  const cwd = getRealpath(
    typeof opts.cwd === 'string' ? opts.cwd : process.env.NYC_CWD || process.cwd(),
  );
  const keys = Object.keys(opts).filter((key) => !OWN_OPTIONS.has(key));
  const locationKeys = keys.filter((key) => LOCATION_ONLY_OPTIONS.has(key));
  if (keys.length > locationKeys.length) {
    return { cwd, ...opts };
  }
  if (locationKeys.length === 0 && process.env.NYC_CONFIG) {
    return { ...parseNycConfig(process.env.NYC_CONFIG), ...pickOwn(opts) };
  }
  return { cwd, ...opts };
};

let nycConfigCache = { text: null, value: null };
// nyc sets one value for the whole process, so parse it once.
const parseNycConfig = (text) => {
  if (nycConfigCache.text !== text) nycConfigCache = { text, value: JSON.parse(text) };
  return nycConfigCache.value;
};

const pickOwn = (opts) =>
  Object.fromEntries(Object.entries(opts).filter(([key]) => OWN_OPTIONS.has(key)));

const toArray = (value) => {
  if (value === undefined || value === null) return undefined;
  return Array.isArray(value) ? value : [value];
};

/**
 * Validates the options and returns them in normal form.
 *
 * @param {Record<string, unknown>} config
 */
const normalizeOptions = (config) => {
  const compat = config.compat === undefined ? 'istanbul' : config.compat;
  if (compat !== 'istanbul' && compat !== null) {
    throw new TypeError(
      `${PLUGIN_NAME}/babel: option "compat" must be 'istanbul' or null, got ${JSON.stringify(config.compat)}`,
    );
  }
  const coverageVariable = config.coverageVariable ?? DEFAULT_COVERAGE_VARIABLE;
  if (typeof coverageVariable !== 'string') {
    throw new TypeError(`${PLUGIN_NAME}/babel: option "coverageVariable" must be a string`);
  }
  const ignoreClassMethods = toArray(config.ignoreClassMethods) ?? [];
  if (!ignoreClassMethods.every((name) => typeof name === 'string')) {
    throw new TypeError(`${PLUGIN_NAME}/babel: option "ignoreClassMethods" must be an array of strings`);
  }
  if (config.onCover !== undefined && typeof config.onCover !== 'function') {
    throw new TypeError(`${PLUGIN_NAME}/babel: option "onCover" must be a function`);
  }
  if (config.onlyWithIstanbul !== undefined && typeof config.onlyWithIstanbul !== 'boolean') {
    throw new TypeError(`${PLUGIN_NAME}/babel: option "onlyWithIstanbul" must be a boolean`);
  }
  // The native instrumenter always finds the global object through
  // `globalThis`. Reject a different scope, so that coverage does not go to an
  // unexpected object without a message.
  if (config.coverageGlobalScope !== undefined && config.coverageGlobalScope !== 'this') {
    throw new TypeError(
      `${PLUGIN_NAME}/babel: option "coverageGlobalScope" is not supported. The coverage object is always on globalThis.`,
    );
  }
  return {
    compat,
    coverageVariable,
    ignoreClassMethods,
    reportLogic: config.reportLogic === true,
    onCover: config.onCover,
    inputSourceMap: config.inputSourceMap,
    useInlineSourceMaps: config.useInlineSourceMaps,
    cwd: config.cwd,
    include: config.include,
    exclude: config.exclude,
    extension: config.extension,
    excludeNodeModules: config.excludeNodeModules,
  };
};

// ---------------------------------------------------------------------------
// include / exclude / extension
// ---------------------------------------------------------------------------

/**
 * Returns a function that tells whether a file must not be instrumented.
 *
 * The plugin uses `test-exclude` when it can load it. That is the package that
 * babel-plugin-istanbul uses, so the result is the same. Jest, nyc and
 * babel-plugin-istanbul all install it. Without `test-exclude`, the plugin uses
 * `path.matchesGlob()` from Node.js.
 */
const makeShouldSkip = () => {
  let cached = null;
  return (file, options) => {
    const key = JSON.stringify([
      options.cwd,
      options.include,
      options.exclude,
      options.extension,
      options.excludeNodeModules,
    ]);
    if (!cached || cached.key !== key) {
      cached = { key, matcher: createMatcher(options) };
    }
    return !cached.matcher(file);
  };
};

const createMatcher = (options) => {
  const settings = {
    cwd: options.cwd,
    include: options.include,
    exclude: options.exclude,
    extension: options.extension,
    excludeNodeModules: options.excludeNodeModules !== false,
  };
  const TestExclude = loadTestExclude();
  if (TestExclude) {
    const exclude = new TestExclude(settings);
    return (file) => exclude.shouldInstrument(file);
  }
  return createFallbackMatcher(settings);
};

const loadTestExclude = () => {
  try {
    return require('test-exclude');
  } catch {
    return null;
  }
};

/**
 * A small version of `test-exclude` for when that package is not installed.
 * It supports glob patterns, `!` negation in `exclude`, the extension filter
 * and the `node_modules` rule.
 */
const createFallbackMatcher = (settings) => {
  if (typeof nodePath.matchesGlob !== 'function') {
    throw new Error(
      `${PLUGIN_NAME}/babel: install "test-exclude" to use include and exclude on this Node.js version`,
    );
  }
  const cwd = settings.cwd;
  const include = toArray(settings.include) ?? [];
  const exclude = [...(toArray(settings.exclude) ?? DEFAULT_EXCLUDE)];
  if (settings.excludeNodeModules && !exclude.includes('**/node_modules/**')) {
    exclude.push('**/node_modules/**');
  }
  const extension =
    settings.extension === false ? [] : (toArray(settings.extension) ?? DEFAULT_EXTENSIONS);
  const matches = (relative, pattern) =>
    nodePath.matchesGlob(relative, pattern) || nodePath.matchesGlob(relative, `${pattern}/**`);
  return (file) => {
    if (!file.startsWith(cwd + nodePath.sep) && file !== cwd) return false;
    const relative = nodePath.relative(cwd, file).split(nodePath.sep).join('/');
    if (extension.length > 0 && !extension.some((ext) => relative.endsWith(ext))) return false;
    if (include.length > 0 && !include.some((pattern) => matches(relative, pattern))) return false;
    let excluded = false;
    for (const pattern of exclude) {
      if (pattern.startsWith('!')) {
        if (matches(relative, pattern.slice(1))) excluded = false;
      } else if (matches(relative, pattern)) {
        excluded = true;
      }
    }
    return !excluded;
  };
};

// ---------------------------------------------------------------------------
// Source map decode and location rewrite
// ---------------------------------------------------------------------------

const BASE64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
const BASE64_VALUE = new Int8Array(128).fill(-1);
for (let i = 0; i < BASE64.length; i++) BASE64_VALUE[BASE64.charCodeAt(i)] = i;

/**
 * Decodes the `mappings` of a source map with one source. The result has one
 * entry for each generated line (0-based). An entry holds the generated
 * columns in ascending order and the original line and column (0-based) of
 * each column.
 *
 * @param {string} mappings
 * @returns {Array<{ columns: number[], lines: number[], origColumns: number[] }>}
 */
const decodeMappings = (mappings) => {
  const result = [];
  let entry = { columns: [], lines: [], origColumns: [] };
  let genColumn = 0;
  let source = 0;
  let origLine = 0;
  let origColumn = 0;
  let index = 0;
  const fields = [0, 0, 0, 0, 0];
  while (index <= mappings.length) {
    const char = mappings.charCodeAt(index);
    if (index === mappings.length || char === 59 /* ; */) {
      result.push(entry);
      entry = { columns: [], lines: [], origColumns: [] };
      genColumn = 0;
      index++;
      continue;
    }
    if (char === 44 /* , */) {
      index++;
      continue;
    }
    let count = 0;
    while (index < mappings.length) {
      const c = mappings.charCodeAt(index);
      if (c === 44 || c === 59) break;
      let value = 0;
      let shift = 0;
      let digit;
      do {
        digit = BASE64_VALUE[mappings.charCodeAt(index++)];
        value += (digit & 31) << shift;
        shift += 5;
      } while (digit & 32);
      fields[count++] = value & 1 ? -(value >>> 1) : value >>> 1;
    }
    genColumn += fields[0];
    if (count >= 4) {
      source += fields[1];
      origLine += fields[2];
      origColumn += fields[3];
      const last = entry.columns.length - 1;
      if (source === 0 && (last === -1 || entry.columns[last] < genColumn)) {
        entry.columns.push(genColumn);
        entry.lines.push(origLine);
        entry.origColumns.push(origColumn);
      }
    }
  }
  return result;
};

/**
 * Returns the index of the greatest value in the sorted `values` that is not
 * greater than `target`, or -1.
 */
const floorIndex = (values, target) => {
  let low = 0;
  let high = values.length - 1;
  let found = -1;
  while (low <= high) {
    const mid = (low + high) >>> 1;
    if (values[mid] <= target) {
      found = mid;
      low = mid + 1;
    } else {
      high = mid - 1;
    }
  }
  return found;
};

/**
 * Returns the offset of the first character of each line in `text`, with the
 * line terminators of the Babel parser.
 *
 * @param {string} text
 * @returns {number[]}
 */
const lineStarts = (text) => {
  const starts = [0];
  for (let i = 0; i < text.length; i++) {
    const c = text.charCodeAt(i);
    if (c === 10) starts.push(i + 1);
    else if (c === 13) {
      if (text.charCodeAt(i + 1) === 10) i++;
      starts.push(i + 1);
    } else if (c === 0x2028 || c === 0x2029) starts.push(i + 1);
  }
  return starts;
};

/**
 * Builds a function that moves one Babel position (1-based line, 0-based
 * column) from the instrumented text to the source text. With `exact`, only a
 * mapping at that exact column counts. Without it, the nearest mapping to the
 * left on the same line counts, plus the column distance. The function returns
 * null when no mapping counts.
 */
const createPositionMapper = (sourceMapJson, originalCode) => {
  const table = decodeMappings(JSON.parse(sourceMapJson).mappings);
  const starts = lineStarts(originalCode);
  return (position, exact) => {
    const entry = table[position.line - 1];
    if (!entry) return null;
    const at = floorIndex(entry.columns, position.column);
    if (at === -1) return null;
    const delta = position.column - entry.columns[at];
    if (exact && delta !== 0) return null;
    const line = entry.lines[at];
    const column = entry.origColumns[at] + delta;
    return { line: line + 1, column, index: (starts[line] ?? 0) + column };
  };
};

/** Moves the location of one node or comment. */
const relocate = (node, mapPosition) => {
  const loc = node.loc;
  if (!loc) return;
  const start = mapPosition(loc.start, true);
  if (!start) {
    node.loc = null;
    node.start = null;
    node.end = null;
    return;
  }
  const mappedEnd = mapPosition(loc.end, false);
  const end = mappedEnd && mappedEnd.index >= start.index ? mappedEnd : start;
  node.loc = {
    start: { line: start.line, column: start.column, index: start.index },
    end: { line: end.line, column: end.column, index: end.index },
    filename: loc.filename,
    identifierName: loc.identifierName,
  };
  node.start = start.index;
  node.end = end.index;
};

/**
 * Moves the locations of every node and comment of `ast` from the
 * instrumented text to the source text. Renames the native coverage function
 * to `coverageFn` in the same walk.
 */
const rewriteLocations = (ast, visitorKeys, mapPosition, nativeFn, coverageFn) => {
  // Nodes share the comment objects, so move each comment once, here.
  for (const comment of ast.comments || []) relocate(comment, mapPosition);
  const stack = [ast];
  while (stack.length > 0) {
    const node = stack.pop();
    relocate(node, mapPosition);
    if (node.type === 'Identifier' && node.name === nativeFn) node.name = coverageFn;
    const keys = visitorKeys[node.type];
    if (!keys) continue;
    for (const key of keys) {
      const child = node[key];
      if (!child) continue;
      if (Array.isArray(child)) {
        for (const item of child) if (item && typeof item.type === 'string') stack.push(item);
      } else if (typeof child.type === 'string') {
        stack.push(child);
      }
    }
  }
};

// ---------------------------------------------------------------------------
// Coverage object
// ---------------------------------------------------------------------------

// istanbul-lib-instrument writes the implicit `else` of an `if` as
// `{ line: undefined, column: undefined }`. JSON drops the keys, but the
// coverage object in the code and `readInitialCoverage()` keep them.
const undefinedPosition = (position) =>
  position && position.line === undefined && position.column === undefined
    ? { line: undefined, column: undefined }
    : position;

const withUndefinedPositions = (location) => ({
  ...location,
  start: undefinedPosition(location.start),
  end: undefinedPosition(location.end),
});

/**
 * Returns the file coverage object with the key order of
 * `istanbul-lib-instrument`, so that `JSON.stringify` of the result is equal
 * to the output of babel-plugin-istanbul.
 */
const toIstanbulKeyOrder = (coverage) => {
  const fnMap = {};
  for (const [id, fn] of Object.entries(coverage.fnMap)) {
    fnMap[id] = { name: fn.name, decl: fn.decl, loc: fn.loc, line: fn.line, ...fn };
  }
  const branchMap = {};
  for (const [id, branch] of Object.entries(coverage.branchMap)) {
    branchMap[id] = {
      loc: branch.loc,
      type: branch.type,
      locations: branch.locations.map(withUndefinedPositions),
      line: branch.line,
    };
    for (const [key, value] of Object.entries(branch)) {
      if (!(key in branchMap[id])) branchMap[id][key] = value;
    }
  }
  return { ...coverage, fnMap, branchMap };
};

/** The hash that babel-plugin-istanbul gives to a coverage object. */
const istanbulHash = (fileCoverage) =>
  createHash('sha1')
    .update(JSON.stringify({ ...fileCoverage, [MAGIC_KEY]: MAGIC_VALUE }))
    .digest('hex');

// The coverage object of a file can hold thousands of nodes. During the
// traversal it stays out of the AST, so that the other plugins and presets do
// not visit it. babel-plugin-istanbul also adds it only at the end. The
// instrumented code holds this small placeholder until `post()`.
const PLACEHOLDER = '{}';

/**
 * Replaces the coverage object in the instrumented code with the placeholder.
 * Only the header line changes, and it has no source map segments, so the
 * source map stays valid.
 */
const replaceCoverageObject = (code, coverageJson) => {
  const at = code.indexOf(coverageJson);
  if (at === -1) {
    throw new Error(`${PLUGIN_NAME}/babel: cannot find the coverage object in the instrumented code`);
  }
  return code.slice(0, at) + PLACEHOLDER + code.slice(at + coverageJson.length);
};

/** Returns the declarator `name` in a list of statements, or undefined. */
const findDeclarator = (statements, name) => {
  for (const statement of statements) {
    if (statement.type !== 'VariableDeclaration') continue;
    for (const declarator of statement.declarations) {
      if (declarator.id.type === 'Identifier' && declarator.id.name === name) return declarator;
    }
  }
  return undefined;
};

/**
 * Finds the `hash` and `coverageData` declarators in the coverage function of
 * an instrumented program.
 */
const findCoverageDeclarators = (program, coverageFn) => {
  const declarator = findDeclarator(program.body, coverageFn);
  let factory = declarator && declarator.init && declarator.init.callee;
  if (factory && factory.type === 'ParenthesizedExpression') factory = factory.expression;
  const body = factory && factory.body && factory.body.body;
  const hash = body && findDeclarator(body, 'hash');
  const data = body && findDeclarator(body, 'coverageData');
  if (!hash || !data) {
    throw new Error(`${PLUGIN_NAME}/babel: cannot find the coverage object in the instrumented program`);
  }
  return { hash, data };
};

/**
 * Writes the final coverage object and its hash into the instrumented AST,
 * in the form of babel-plugin-istanbul: the same key order, the
 * `inputSourceMap` when the file has one, and the schema key as an
 * identifier. `readInitialCoverage()` accepts the schema key only as an
 * identifier.
 */
const writeCoverageObject = (t, declarators, fileCoverage) => {
  declarators.data.init = t.valueToNode({ ...fileCoverage, [MAGIC_KEY]: MAGIC_VALUE });
  declarators.hash.init = t.stringLiteral(istanbulHash(fileCoverage));
};

// ---------------------------------------------------------------------------
// Instrumentation
// ---------------------------------------------------------------------------

const TS_EXTENSIONS = new Set(['.ts', '.mts', '.cts']);

/** Returns the names of the Babel parser plugins. */
const parserPluginNames = (parserOpts) => {
  const plugins = parserOpts && Array.isArray(parserOpts.plugins) ? parserOpts.plugins : [];
  return new Set(plugins.map((plugin) => (Array.isArray(plugin) ? plugin[0] : plugin)));
};

/**
 * Returns the `sourceType` for the native parser, or undefined.
 *
 * The native parser selects the language from the file extension. Babel
 * selects it from the parser plugins. These two can be different, for example
 * JSX in a `.js` file with the `jsx` parser plugin (preset-react). Then the
 * plugin gives the language of the Babel parser to the native parser. When the
 * extension already gives that language, the plugin keeps the extension.
 *
 * `allowReturnOutsideFunction` without JSX or TypeScript maps to `commonjs`,
 * because only that native source type accepts `return` at the top level.
 *
 * @param {string} filename
 * @param {Record<string, unknown> | undefined} parserOpts
 * @returns {'jsx' | 'ts' | 'tsx' | 'commonjs' | undefined}
 */
const nativeSourceType = (filename, parserOpts) => {
  const plugins = parserPluginNames(parserOpts);
  const typescript = plugins.has('typescript');
  const jsx = plugins.has('jsx');
  const ext = nodePath.extname(filename.split(/[?#]/)[0]).toLowerCase();
  if (typescript && jsx) return ext === '.tsx' ? undefined : 'tsx';
  if (typescript) return TS_EXTENSIONS.has(ext) ? undefined : 'ts';
  if (jsx) return ext === '.jsx' ? undefined : 'jsx';
  if (parserOpts && parserOpts.allowReturnOutsideFunction === true) return 'commonjs';
  return undefined;
};

/**
 * Calls the native `instrument()`. A parse error names the file and the
 * syntax that the native parser does not support.
 */
const instrumentSource = (code, filename, nativeOptions) => {
  try {
    return instrument(code, filename, nativeOptions);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    if (!/parse error/i.test(message)) throw error;
    throw new Error(
      `${PLUGIN_NAME}/babel: cannot parse ${filename}: ${message}. ` +
        'The native parser does not parse Flow or some syntax proposals, and it does ' +
        'not support `allowReturnOutsideFunction` together with JSX or TypeScript.',
      { cause: error },
    );
  }
};

/**
 * Instruments the source text of one file and parses the result. For a file
 * with `istanbul ignore file`, `ast` is null.
 *
 * @returns {{ ast: object, fileCoverage: object, declarators: object } | { ast: null, fileCoverage: object }}
 */
const instrumentToAst = (t, code, filename, options, parse, parserOpts) => {
  const result = instrumentSource(code, filename, {
    compat: options.compat ?? undefined,
    coverageVariable: options.coverageVariable,
    ignoreClassMethods: options.ignoreClassMethods,
    reportLogic: options.reportLogic,
    sourceMap: true,
    sourceType: nativeSourceType(filename, parserOpts),
  });
  const coverageJson = result.coverageMap;
  const fileCoverage = toIstanbulKeyOrder(JSON.parse(coverageJson));
  let instrumented = result.code.toString();
  const nativeFn = NATIVE_COVERAGE_FN_RE.exec(instrumented);
  if (!nativeFn) return { ast: null, fileCoverage };

  instrumented = replaceCoverageObject(instrumented, coverageJson);
  const ast = parse(instrumented, parserOpts);
  const coverageFn = istanbulCoverageFunctionName(filename);
  rewriteLocations(
    ast,
    t.VISITOR_KEYS,
    createPositionMapper(result.sourceMap, code),
    nativeFn[1],
    coverageFn,
  );
  const declarators = findCoverageDeclarators(ast.program, coverageFn);
  return { ast, fileCoverage, declarators };
};

/**
 * Finds babel-plugin-istanbul in the plugin list of the current file, and
 * returns its options. Returns undefined when it is not in the list.
 */
const findIstanbulPlugin = (file) => {
  const plugins = file.opts && Array.isArray(file.opts.plugins) ? file.opts.plugins : [];
  for (const plugin of plugins) {
    const key = plugin && typeof plugin.key === 'string' ? plugin.key : '';
    if (/babel-plugin-istanbul/.test(key) || key === 'istanbul') {
      return plugin.options || {};
    }
  }
  return undefined;
};

/**
 * Stops with a clear message when babel-plugin-istanbul in the same run has an
 * `onCover` option. For a file that this plugin instrumented, the Program exit
 * of babel-plugin-istanbul reads `fileCoverage` of an undefined result when it
 * calls its `onCover`, and Babel stops with a TypeError. The defect is in
 * babel-plugin-istanbul, but this plugin causes the state, so it names the
 * cause first.
 */
const assertNoIstanbulOnCover = (file) => {
  const istanbulOptions = findIstanbulPlugin(file);
  if (istanbulOptions && typeof istanbulOptions.onCover === 'function') {
    throw new Error(
      `${PLUGIN_NAME}/babel: babel-plugin-istanbul in the same Babel run has an "onCover" option. ` +
        'It cannot read the coverage of a file that this plugin instrumented. ' +
        'Give "onCover" to oxc-coverage-instrument/babel, or remove babel-plugin-istanbul.',
    );
  }
};

/** Returns the input source map that babel-plugin-istanbul would attach. */
const resolveInputSourceMap = (options, file) => {
  if (options.inputSourceMap) return options.inputSourceMap;
  if (options.useInlineSourceMaps !== false && file.inputMap) return file.inputMap.sourcemap;
  return undefined;
};

/**
 * Resolves the options and tells whether the plugin instruments `filename`.
 * Returns null when it does not.
 */
const resolveFile = (opts, filename, shouldSkip) => {
  const config = findConfig(opts);
  const options = normalizeOptions(config);
  if (typeof filename !== 'string' || filename === '') return null;
  const realPath = getRealpath(filename);
  if (shouldSkip(realPath, { ...options, cwd: config.cwd })) return null;
  return { options, realPath };
};

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

/**
 * The Babel plugin. Babel calls it with its plugin API object and the plugin
 * options.
 *
 * @param {object} api The Babel plugin API.
 * @param {import('./babel.d.ts').OxcBabelPluginOptions} [pluginOptions]
 * @returns {object} The Babel plugin object.
 */
const babelPluginOxcCoverage = (api, pluginOptions) => {
  if (api && typeof api.assertVersion === 'function') {
    api.assertVersion('^7.0.0 || ^8.0.0-0');
  }
  const t = api && api.types;
  if (!t || !t.VISITOR_KEYS) {
    throw new Error(`${PLUGIN_NAME}/babel: the Babel plugin API has no types`);
  }
  const ownOptions = pluginOptions || {};
  // Validate the options when Babel loads the plugin, not at the first file.
  normalizeOptions(findConfig(ownOptions));
  const shouldSkip = makeShouldSkip();
  // The result of `parserOverride` for each AST it returned.
  const parsed = new WeakMap();
  // Source text of files that `parserOverride` instrumented without a result
  // (`istanbul ignore file`), by file name. `Program.enter` skips them.
  const declined = new Map();
  let babelParse = null;

  return {
    name: PLUGIN_NAME,

    parserOverride(code, parserOpts, parse) {
      babelParse = parse;
      if (ownOptions.onlyWithIstanbul) return undefined;
      const filename = parserOpts.sourceFileName;
      const target = resolveFile(ownOptions, filename, shouldSkip);
      if (!target) return undefined;
      const result = instrumentToAst(t, code, target.realPath, target.options, parse, parserOpts);
      if (!result.ast) {
        declined.set(filename, code);
        return undefined;
      }
      parsed.set(result.ast, { ...target, ...result });
      return result.ast;
    },

    visitor: {
      Program: {
        enter(path, state) {
          state.oxcCoverage = null;
          const file = state.file;
          const done = parsed.get(file.ast);
          if (done) {
            parsed.delete(file.ast);
            state.oxcCoverage = done;
            assertNoIstanbulOnCover(file);
            return;
          }
          const filename = file.opts.filename;
          if (declined.has(filename) && declined.get(filename) === file.code) {
            declined.delete(filename);
            return;
          }

          // `parserOverride` did not run (`onlyWithIstanbul`, or
          // `transformFromAst`). Instrument the source text here, and replace
          // the program body.
          let opts = ownOptions;
          if (ownOptions.onlyWithIstanbul) {
            const istanbulOptions = findIstanbulPlugin(file);
            if (!istanbulOptions) return;
            // babel-plugin-istanbul cannot use its `onCover` here (see
            // `assertNoIstanbulOnCover`), so do not copy it.
            const { onCover: _istanbulOnCover, ...rest } = istanbulOptions;
            opts = { ...rest, ...ownOptions };
          }
          if (typeof file.code !== 'string' || file.code === '') return;
          const target = resolveFile(opts, filename, shouldSkip);
          if (!target) return;
          // `transformFromAst` does not call `parserOverride`, so the Babel
          // parser function can be unknown here.
          const parse =
            babelParse ??
            ((code, parserOpts) =>
              api.parseSync(code, { babelrc: false, configFile: false, parserOpts }));
          const result = instrumentToAst(t, file.code, target.realPath, target.options, parse, {
            ...file.opts.parserOpts,
          });
          if (!result.ast) return;
          const program = path.node;
          program.body = result.ast.program.body;
          program.directives = result.ast.program.directives;
          if (file.ast) file.ast.comments = result.ast.comments;
          path.scope.crawl();
          state.oxcCoverage = { ...target, ...result };
          assertNoIstanbulOnCover(file);
        },
      },
    },

    // Babel calls `post()` after the traversal, before it generates code.
    post(file) {
      const done = this.oxcCoverage;
      if (!done) return;
      this.oxcCoverage = null;
      const inputSourceMap = resolveInputSourceMap(done.options, file);
      if (inputSourceMap) done.fileCoverage.inputSourceMap = inputSourceMap;
      writeCoverageObject(t, done.declarators, done.fileCoverage);
      file.metadata.coverage = done.fileCoverage;
      if (done.options.onCover) done.options.onCover(done.realPath, done.fileCoverage);
    },
  };
};

module.exports = babelPluginOxcCoverage;
module.exports.default = babelPluginOxcCoverage;
module.exports.babelPluginOxcCoverage = babelPluginOxcCoverage;
