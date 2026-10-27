import type {
  IstanbulFileCoverage,
  JsonObject,
  OxcInputSourceMap,
  OxcInstrumenterOptions,
  OxcSourceMap,
} from './vitest';

/**
 * Options for the karma-coverage instrumenter. Set them in
 * `coverageReporter.instrumenterOptions.<name>`.
 *
 * karma-coverage also sends `autoWrap`, `codeGenerationOptions`, and other
 * `istanbul-lib-instrument` options. The adapter accepts them and does not use
 * them.
 */
export interface OxcKarmaInstrumenterOptions extends OxcInstrumenterOptions {
  /**
   * Return the source map from `lastSourceMap()`. karma-coverage sets this to
   * true when the input file has a source map. Defaults to false, like
   * `istanbul-lib-instrument`.
   */
  produceSourceMap?: boolean;
  /** Other `istanbul-lib-instrument` options. The adapter does not use them. */
  [option: string]: unknown;
}

/** Callback that karma-coverage gives to `instrument()`. */
export type OxcKarmaInstrumentCallback = (error: Error | null, code?: string) => void;

/**
 * Instrumenter with the shape that karma-coverage expects from an entry in
 * `coverageReporter.instrumenters`.
 *
 * @example
 * ```js
 * // karma.conf.js
 * module.exports = (config) => {
 *   config.set({
 *     preprocessors: { 'src/**\/*.js': ['coverage'] },
 *     reporters: ['progress', 'coverage'],
 *     coverageReporter: {
 *       instrumenters: { oxc: require('oxc-coverage-instrument/karma') },
 *       // karma-coverage matches these patterns against the absolute path
 *       // of each file. Start each pattern with `**\/`. A pattern that does
 *       // not match selects the istanbul instrumenter without a warning.
 *       instrumenter: { '**\/*.js': 'oxc' },
 *       instrumenterOptions: { oxc: { compat: 'istanbul' } },
 *     },
 *   });
 * };
 * ```
 */
export declare class Instrumenter {
  constructor(options?: OxcKarmaInstrumenterOptions);
  /** Instruments `code` and returns the instrumented source. */
  instrumentSync(
    code: string,
    filename: string,
    inputSourceMap?: OxcInputSourceMap | JsonObject,
  ): string;
  /** Callback form that karma-coverage calls. */
  instrument(
    code: string,
    filename: string,
    callback: OxcKarmaInstrumentCallback,
    inputSourceMap?: OxcInputSourceMap | JsonObject,
  ): void;
  instrument(code: string, callback: OxcKarmaInstrumentCallback): void;
  /** Source map of the last file, or `null` when `produceSourceMap` is not true. */
  lastSourceMap(): OxcSourceMap | null;
  /** Coverage object of the last instrumented file. */
  lastFileCoverage(): IstanbulFileCoverage | null;
}

/** Creates a karma-coverage instrumenter. Equal to `new Instrumenter(options)`. */
export declare function createKarmaInstrumenter(options?: OxcKarmaInstrumenterOptions): Instrumenter;
