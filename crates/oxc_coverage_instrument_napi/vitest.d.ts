/**
 * Options for creating an Oxc-based coverage instrumenter.
 *
 * When used with Vitest's `coverage.instrumenter` option, the factory receives
 * `InstrumenterOptions` with `coverageVariable` and `ignoreClassMethods`.
 */
export interface OxcInstrumenterOptions {
  /** Match the `istanbul-lib-instrument` coverage shape. */
  compat?: 'istanbul';
  /** Global variable name for coverage data (Vitest passes `__VITEST_COVERAGE__`). */
  coverageVariable?: string;
  /** Class method names to exclude from coverage instrumentation. */
  ignoreClassMethods?: string[];
  /** When true, adds truthy-value tracking (bT) for logical expressions. */
  reportLogic?: boolean;
  /**
   * Track receiver-safe optional chaining (`?.`) links as branches. Defaults
   * to true. Receiver-bound optional calls stay native to preserve `this`.
   * Set to false to leave every optional chain native, matching
   * `istanbul-lib-instrument`.
   */
  trackOptionalChainBranches?: boolean;
  /**
   * Attach the optional `x_fallow_functionMap` extension to the last file
   * coverage object. Defaults to false.
   */
  functionIdentityOverlay?: boolean;
  /**
   * Name an otherwise-anonymous function/arrow that is a direct call/`new`
   * argument from the callee (`arr.map(cb)` -> `"map"`), instead of the
   * `(anonymous_N)` fallback. Binding names still take precedence. Defaults to
   * false (byte-identical to Istanbul).
   */
  nameCallbackArguments?: boolean;
  /**
   * Record the consequent (`{ b(); }` in `if (a) { b(); } else { c(); }`) as
   * the location of arm 0 of an `if` branch, instead of the whole `if`
   * statement. Arm 1, the branch `loc`, and the counters do not change.
   * `compat: 'istanbul'` ignores it. Defaults to false (the Istanbul shape).
   */
  ifConsequentLocation?: boolean;
  /**
   * Run the TypeScript-strip pass before instrumentation. When omitted
   * (default), the adapter auto-detects: it strips when the filename matches
   * `/\.([mc]ts|tsx?)$/i` AND no `inputSourceMap` was supplied (i.e., the
   * source has not already been transformed by Vite / Babel / tsc). Set to
   * `false` to disable auto-detect under toolchains that pre-transform
   * TypeScript but do not produce an `inputSourceMap` (`@vitejs/plugin-react-swc`
   * in some configurations, Bun's native TS runner, Node 23+ with
   * `--experimental-strip-types`). Set to `true` to force strip. Non-boolean
   * values throw `TypeError`.
   */
  stripTypescript?: boolean;
  /**
   * Lower legacy `experimentalDecorators` syntax into `_decorate(...)` calls.
   * Defaults to false and only applies when the TypeScript strip pass runs.
   * Requires `@oxc-project/runtime` at execution; see the README's "Legacy
   * decorators" section. This option is not detected from `tsconfig.json` and
   * must be enabled explicitly.
   */
  experimentalDecorators?: boolean;
  /**
   * Emit `_decorateMetadata` type metadata alongside decorated members.
   * Required for NestJS dependency injection and TypeORM column inference.
   * Setting this to true auto-promotes `experimentalDecorators` to true at
   * this adapter layer, matching `tsconfig.json` semantics.
   */
  emitDecoratorMetadata?: boolean;
  /**
   * Whether the source is compiled under `strictNullChecks`, matching the
   * `tsconfig.json` flag. Only consulted when `emitDecoratorMetadata` is true,
   * where it decides how a nullable union is written into `design:type`:
   * `foo: string | null` emits `Object` when true and `String` when false.
   * Defaults to true, matching `tsc` under `strict`.
   */
  strictNullChecks?: boolean;
}

/** A JSON primitive accepted in an input source map. */
export type JsonPrimitive = string | number | boolean | null;

/** A JSON-compatible value accepted in an input source map. */
export type JsonValue = JsonPrimitive | JsonObject | JsonValue[];

/** A JSON-compatible object accepted as an input source map. */
export interface JsonObject {
  [key: string]: JsonValue;
}

/** A structurally typed encoded source map accepted by the instrumenter. */
export interface OxcInputSourceMap {
  version: string | number;
  sources: Array<string | null>;
  names?: string[];
  mappings: string;
  file?: string | null;
  sourceRoot?: string;
  sourcesContent?: Array<string | null>;
  ignoreList?: number[];
  x_google_ignoreList?: number[];
}

/** A source map returned by the instrumenter. */
export interface OxcSourceMap {
  [key: string]: JsonValue | undefined;
  version: 3;
  sources: Array<string | null>;
  names: string[];
  mappings: string;
  file?: string | null;
  sourceRoot?: string;
  sourcesContent?: Array<string | null>;
  ignoreList?: number[];
  x_google_ignoreList?: number[];
}

/** A position in an Istanbul coverage location. */
export interface IstanbulPosition {
  line?: number;
  column?: number;
}

/** A source range in Istanbul coverage data. */
export interface IstanbulLocation {
  start: IstanbulPosition;
  end: IstanbulPosition;
}

/** Function metadata in Istanbul coverage data. */
export interface IstanbulFunction {
  name: string;
  line: number;
  decl: IstanbulLocation;
  loc: IstanbulLocation;
}

/** Branch metadata in Istanbul coverage data. */
export interface IstanbulBranch {
  loc: IstanbulLocation;
  line: number;
  type: string;
  locations: IstanbulLocation[];
}

/** Stable function identity metadata attached by the optional overlay. */
export interface IstanbulFunctionIdentity {
  id: string;
  name: string;
  path: string;
  decl: IstanbulLocation;
  loc: IstanbulLocation;
}

/** Coverage data for one instrumented file. */
export interface IstanbulFileCoverage {
  path: string;
  statementMap: Record<string, IstanbulLocation>;
  fnMap: Record<string, IstanbulFunction>;
  branchMap: Record<string, IstanbulBranch>;
  s: Record<string, number>;
  f: Record<string, number>;
  b: Record<string, number[]>;
  bT?: Record<string, number[]>;
  inputSourceMap?: JsonValue;
  x_fallow_functionMap?: Record<string, IstanbulFunctionIdentity>;
}

/** Instrumenter returned by `createOxcInstrumenter`. */
export interface OxcInstrumenter {
  instrumentSync(
    code: string,
    filename: string,
    inputSourceMap?: OxcInputSourceMap | JsonObject,
  ): string;
  lastSourceMap(): OxcSourceMap | null;
  lastFileCoverage(): IstanbulFileCoverage | null;
  /** Property alias for compatibility with vite-plugin-istanbul. */
  readonly fileCoverage: IstanbulFileCoverage | null;
}

/**
 * Creates an instrumenter that implements the istanbul-lib-instrument
 * Instrumenter interface, backed by oxc-coverage-instrument.
 *
 * @example
 * ```ts
 * import { defineConfig } from 'vitest/config'
 * import { createOxcInstrumenter } from 'oxc-coverage-instrument/vitest'
 *
 * export default defineConfig({
 *   test: {
 *     coverage: {
 *       provider: 'istanbul',
 *       instrumenter: (options) => createOxcInstrumenter(options),
 *     }
 *   }
 * })
 * ```
 */
export declare function createOxcInstrumenter(options?: OxcInstrumenterOptions): OxcInstrumenter;
