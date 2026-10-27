// Vitest Istanbul instrumenter adapter for oxc-coverage-instrument.
//
// Implements the istanbul-lib-instrument Instrumenter interface so it can be
// used as a drop-in replacement in @vitest/coverage-istanbul.
//
// Usage in vitest.config.ts:
//
//   import { defineConfig } from 'vitest/config'
//   import { createOxcInstrumenter } from 'oxc-coverage-instrument/vitest'
//
//   export default defineConfig({
//     test: {
//       coverage: {
//         provider: 'istanbul',
//         instrumenter: (options) => createOxcInstrumenter(options),
//       }
//     }
//   })

const { instrument } = require('./index.js');

// Filename pattern matching exactly the TypeScript extensions Node recognises:
// .ts, .tsx, .mts, .cts. The narrower form `/\.([mc]ts|tsx?)$/i` is preferred
// over `/\.[mc]?tsx?$/i` because the latter would also match `.mtsx` and
// `.ctsx`, which are not real TypeScript extensions. Used for auto-detect;
// explicit user opt-in/out via the `stripTypescript` option always wins.
const TS_EXTENSION_REGEX = /\.([mc]ts|tsx?)$/i;

/**
 * Creates an instrumenter that implements the istanbul-lib-instrument
 * Instrumenter interface, backed by oxc-coverage-instrument.
 *
 * When used with Vitest's `coverage.instrumenter` option, the factory receives
 * `InstrumenterOptions` with `coverageVariable` and `ignoreClassMethods`.
 * These are forwarded to the native instrumenter automatically.
 *
 * @param {import('./vitest.d.ts').OxcInstrumenterOptions} [options] Every
 *   option is documented on `OxcInstrumenterOptions` in `vitest.d.ts`.
 * @returns {{ instrumentSync, lastSourceMap, lastFileCoverage }}
 */
function createOxcInstrumenter(options) {
  options = options || {};
  const compat = options.compat;
  if (compat !== undefined && compat !== 'istanbul') {
    throw new TypeError(
      `oxc-coverage-instrument: createOxcInstrumenter({ compat }) must be 'istanbul' or undefined, got ${JSON.stringify(compat)}`,
    );
  }
  const coverageVariable = options.coverageVariable || '__coverage__';
  const ignoreClassMethods = options.ignoreClassMethods || [];
  const reportLogic = options.reportLogic || false;
  // Defaults to true (track), so a bare `|| true` would wrongly coerce an
  // explicit `false` back to `true`. Treat only `undefined` as "use the
  // default" and validate everything else as a strict boolean.
  const trackOptionalChainBranchesRaw = options.trackOptionalChainBranches;
  if (
    trackOptionalChainBranchesRaw !== undefined &&
    typeof trackOptionalChainBranchesRaw !== 'boolean'
  ) {
    throw new TypeError(
      `oxc-coverage-instrument: createOxcInstrumenter({ trackOptionalChainBranches }) must be a boolean or undefined, got ${typeof options.trackOptionalChainBranches}`,
    );
  }
  const trackOptionalChainBranches = trackOptionalChainBranchesRaw ?? compat !== 'istanbul';
  const functionIdentityOverlay = options.functionIdentityOverlay || false;
  if (typeof functionIdentityOverlay !== 'boolean') {
    throw new TypeError(
      `oxc-coverage-instrument: createOxcInstrumenter({ functionIdentityOverlay }) must be a boolean or undefined, got ${typeof options.functionIdentityOverlay}`,
    );
  }
  const nameCallbackArguments = options.nameCallbackArguments || false;
  if (typeof nameCallbackArguments !== 'boolean') {
    throw new TypeError(
      `oxc-coverage-instrument: createOxcInstrumenter({ nameCallbackArguments }) must be a boolean or undefined, got ${typeof options.nameCallbackArguments}`,
    );
  }
  const ifConsequentLocation = options.ifConsequentLocation || false;
  if (typeof ifConsequentLocation !== 'boolean') {
    throw new TypeError(
      `oxc-coverage-instrument: createOxcInstrumenter({ ifConsequentLocation }) must be a boolean or undefined, got ${typeof options.ifConsequentLocation}`,
    );
  }
  // Strictly boolean: a caller passing a string like 'auto' would otherwise be
  // coerced to Boolean('auto') === true, force-stripping every file.
  const stripTypescriptOverride = options.stripTypescript;
  if (stripTypescriptOverride !== undefined && typeof stripTypescriptOverride !== 'boolean') {
    throw new TypeError(
      `oxc-coverage-instrument: createOxcInstrumenter({ stripTypescript }) must be a boolean or undefined, got ${typeof stripTypescriptOverride}`,
    );
  }
  const experimentalDecoratorsRaw = options.experimentalDecorators || false;
  if (typeof experimentalDecoratorsRaw !== 'boolean') {
    throw new TypeError(
      `oxc-coverage-instrument: createOxcInstrumenter({ experimentalDecorators }) must be a boolean or undefined, got ${typeof options.experimentalDecorators}`,
    );
  }
  const emitDecoratorMetadata = options.emitDecoratorMetadata || false;
  if (typeof emitDecoratorMetadata !== 'boolean') {
    throw new TypeError(
      `oxc-coverage-instrument: createOxcInstrumenter({ emitDecoratorMetadata }) must be a boolean or undefined, got ${typeof options.emitDecoratorMetadata}`,
    );
  }
  const strictNullChecks =
    options.strictNullChecks === undefined ? true : options.strictNullChecks;
  if (typeof strictNullChecks !== 'boolean') {
    throw new TypeError(
      `oxc-coverage-instrument: createOxcInstrumenter({ strictNullChecks }) must be a boolean or undefined, got ${typeof options.strictNullChecks}`,
    );
  }
  // Auto-promote at the vitest layer: setting emitDecoratorMetadata alone is a
  // tsconfig.json idiom (TypeScript itself silently enables experimentalDecorators
  // in that case). The bare napi `instrument()` rejects the same combination so
  // the underlying Rust DecoratorMode enum stays unrepresentable-invalid; this
  // adapter is the place to mirror tsconfig ergonomics for vitest users.
  const experimentalDecorators = experimentalDecoratorsRaw || emitDecoratorMetadata;

  // Raw JSON strings from the last instrument call, parsed lazily on first access.
  let _lastCoverageMapJson = null;
  let _lastSourceMapJson = null;
  // Parsed objects, null until actually read via lastFileCoverage()/lastSourceMap().
  let _lastFileCoverage = null;
  let _lastSourceMap = null;
  // Set after each instrumentSync so the next read reparses instead of serving
  // a stale object.
  let _coverageDirty = false;
  let _sourceMapDirty = false;

  function ensureCoverageParsed() {
    if (_coverageDirty && _lastCoverageMapJson !== null) {
      _lastFileCoverage = JSON.parse(_lastCoverageMapJson);
      _coverageDirty = false;
    }
  }

  function ensureSourceMapParsed() {
    if (_sourceMapDirty && _lastSourceMapJson !== null) {
      _lastSourceMap = JSON.parse(_lastSourceMapJson);
      _sourceMapDirty = false;
    }
  }

  return {
    instrumentSync(code, filename, inputSourceMap) {
      const stripTypescript =
        stripTypescriptOverride === undefined
          ? TS_EXTENSION_REGEX.test(filename) && !inputSourceMap
          : stripTypescriptOverride;
      const result = instrument(code, filename, {
        compat,
        coverageVariable,
        sourceMap: true,
        inputSourceMap: inputSourceMap ? JSON.stringify(inputSourceMap) : undefined,
        reportLogic,
        trackOptionalChainBranches,
        ignoreClassMethods,
        stripTypescript,
        experimentalDecorators,
        emitDecoratorMetadata,
        strictNullChecks,
        functionIdentityOverlay,
        nameCallbackArguments,
        ifConsequentLocation,
      });

      _lastCoverageMapJson = result.coverageMap;
      _lastSourceMapJson = result.sourceMap || null;
      _coverageDirty = true;
      _sourceMapDirty = true;

      // result.code is a Buffer (zero-copy from Rust); convert to string for
      // istanbul-lib-instrument compatibility (instrumentSync must return string).
      return result.code.toString();
    },

    lastSourceMap() {
      ensureSourceMapParsed();
      return _lastSourceMap;
    },

    lastFileCoverage() {
      ensureCoverageParsed();
      return _lastFileCoverage;
    },

    // Property alias used by vite-plugin-istanbul (vs lastFileCoverage() method used by Vitest)
    get fileCoverage() {
      ensureCoverageParsed();
      return _lastFileCoverage;
    },
  };
}

module.exports = { createOxcInstrumenter };
