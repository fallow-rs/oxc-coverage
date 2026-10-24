#!/usr/bin/env node
// Time each package that oxc-coverage replaces against its replacement in the
// `oxc-coverage-instrument` npm package, on the pinned real-world corpus from
// `scripts/real-world-corpus.json`.
//
// Stages, in the order of a coverage run:
//   instrument  istanbul-lib-instrument, @vitest/istanbul-lib-instrument
//   v8          v8-to-istanbul, ast-v8-to-istanbul (with an oxc-parser AST)
//   remap       istanbul-lib-source-maps, @vitest/istanbul-lib-source-maps
//   summary     istanbul-lib-coverage, @vitest/istanbul-lib-coverage
//   report      istanbul-lib-report + istanbul-reports, @vitest/istanbul-lib-report
//
// Every tool runs in this one Node.js process. Each measurement is the median
// of --runs runs after two warm-up runs. The comparison packages install at
// the exact versions in PACKAGES into .bench-tmp/replacements.
//
// Usage:
//   node scripts/prepare-real-world-corpus.mjs
//   npm --prefix crates/oxc_coverage_instrument_napi run build   # release build
//   node scripts/replacement-benchmarks.mjs [--runs=7] [--markdown=<file>]

import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import inspector from 'node:inspector';
import { createRequire } from 'node:module';
import os from 'node:os';
import { dirname, join } from 'node:path';
import { performance } from 'node:perf_hooks';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { promisify } from 'node:util';
import vm from 'node:vm';

import { loadVerifiedCorpus } from './prepare-real-world-corpus.mjs';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const INSTALL_DIR = join(ROOT, '.bench-tmp', 'replacements');
const WARMUP_RUNS = 2;
const DEFAULT_RUNS = 7;
const TEXT_FORMATS = ['text', 'text-summary', 'json-summary', 'lcov', 'cobertura'];
// The Istanbul `lcov` report also writes an HTML tree. Its `lcovonly` report
// writes the same `lcov.info` as the oxc `lcov` format, and nothing else.
const ISTANBUL_FORMAT = { lcov: 'lcovonly' };

const PACKAGES = {
  'istanbul-lib-instrument': '6.0.3',
  'istanbul-lib-coverage': '3.2.2',
  'istanbul-lib-source-maps': '5.0.6',
  'istanbul-lib-report': '3.0.1',
  'istanbul-reports': '3.2.0',
  'v8-to-istanbul': '9.3.0',
  'ast-v8-to-istanbul': '1.0.7',
  '@vitest/istanbul-lib-instrument': '2.0.0',
  '@vitest/istanbul-lib-coverage': '2.0.0',
  '@vitest/istanbul-lib-source-maps': '2.0.0',
  '@vitest/istanbul-lib-report': '2.0.0',
  'oxc-parser': '0.155.0',
  '@babel/core': '7.29.7',
};

const parseArgs = () => {
  const args = { runs: DEFAULT_RUNS, markdown: null };
  for (const arg of process.argv.slice(2)) {
    const [key, value] = arg.replace(/^--/, '').split('=');
    if (key === 'runs') args.runs = Number(value);
    else if (key === 'markdown') args.markdown = value;
    else throw new Error(`unknown argument: ${arg}`);
  }
  if (!Number.isInteger(args.runs) || args.runs < 1) throw new Error('--runs must be a positive integer');
  return args;
};

const installPackages = () => {
  const stamp = join(INSTALL_DIR, 'versions.json');
  const wanted = JSON.stringify(PACKAGES);
  if (existsSync(stamp) && readFileSync(stamp, 'utf8') === wanted) return;
  rmSync(INSTALL_DIR, { recursive: true, force: true });
  mkdirSync(INSTALL_DIR, { recursive: true });
  writeFileSync(join(INSTALL_DIR, 'package.json'), '{"private":true}\n');
  const specs = Object.entries(PACKAGES).map(([name, version]) => `${name}@${version}`);
  execFileSync('npm', ['install', '--silent', '--no-audit', '--no-fund', ...specs], {
    cwd: INSTALL_DIR,
    stdio: 'inherit',
  });
  writeFileSync(stamp, wanted);
};

const median = (values) => {
  const sorted = [...values].sort((a, b) => a - b);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
};

const measure = async (runs, task) => {
  for (let i = 0; i < WARMUP_RUNS; i++) await task();
  const times = [];
  for (let i = 0; i < runs; i++) {
    const start = performance.now();
    await task();
    times.push(performance.now() - start);
  }
  return median(times);
};

const counts = (fileCoverage) =>
  [fileCoverage.statementMap, fileCoverage.fnMap, fileCoverage.branchMap]
    .map((map) => Object.keys(map).length)
    .join('/');

// Run a library build once in a fresh context with precise coverage on, and
// return the V8 functions for its URL.
const captureV8 = async (source, url) => {
  const session = new inspector.Session();
  const post = promisify(session.post).bind(session);
  session.connect();
  try {
    await post('Profiler.enable');
    await post('Profiler.startPreciseCoverage', { callCount: true, detailed: true });
    const context = vm.createContext({ console, setTimeout, clearTimeout, module: { exports: {} } });
    context.window = context;
    context.self = context;
    context.exports = context.module.exports;
    try {
      new vm.Script(source, { filename: url }).runInContext(context);
    } catch {
      // A browser build can stop at a missing DOM API. The coverage up to
      // that point is still real V8 output.
    }
    const { result } = await post('Profiler.takePreciseCoverage');
    const script = result.find((entry) => entry.url === url);
    if (!script) throw new Error(`no precise coverage for ${url}`);
    return script.functions;
  } finally {
    await post('Profiler.stopPreciseCoverage').catch(() => undefined);
    await post('Profiler.disable').catch(() => undefined);
    session.disconnect();
  }
};

const main = async () => {
  const { runs, markdown } = parseArgs();
  installPackages();
  const req = createRequire(join(INSTALL_DIR, 'package.json'));
  const load = (name) => import(pathToFileURL(req.resolve(name)).href);

  const oxc = createRequire(import.meta.url)('../crates/oxc_coverage_instrument_napi/index.js');
  const istanbulInstrument = req('istanbul-lib-instrument');
  const istanbulCoverage = req('istanbul-lib-coverage');
  const istanbulSourceMaps = req('istanbul-lib-source-maps');
  const istanbulReport = req('istanbul-lib-report');
  const istanbulReports = req('istanbul-reports');
  const v8ToIstanbul = req('v8-to-istanbul');
  const babel = req('@babel/core');
  const { parseSync } = req('oxc-parser');
  const { convert: astV8ToIstanbul } = await load('ast-v8-to-istanbul');
  const vitestInstrument = await load('@vitest/istanbul-lib-instrument');
  const vitestCoverage = await load('@vitest/istanbul-lib-coverage');
  const vitestSourceMaps = await load('@vitest/istanbul-lib-source-maps');
  const vitestReport = await load('@vitest/istanbul-lib-report');

  const corpus = loadVerifiedCorpus();
  const files = corpus.map((project) => ({
    label: `${project.name} ${project.version}`,
    path: project.path,
    source: readFileSync(project.path, 'utf8'),
  }));
  const scratch = mkdtempSync(join(os.tmpdir(), 'oxc-coverage-bench-'));
  const rows = { instrument: [], v8: [], remap: [] };
  const coverageData = {};
  const remapInput = {};

  for (const file of files) {
    process.stderr.write(`${file.label}\n`);
    const kb = (file.source.length / 1024).toFixed(0);

    // Instrument the library build as a script.
    const istanbulInstrumenter = istanbulInstrument.createInstrumenter({ esModules: false });
    const vitestInstrumenter = vitestInstrument.createInstrumenter({ esModules: false });
    const instrumentTimes = {
      istanbul: await measure(runs, () => istanbulInstrumenter.instrumentSync(file.source, file.path)),
      vitest: await measure(runs, () => vitestInstrumenter.instrumentSync(file.source, file.path)),
      oxc: await measure(runs, () => oxc.instrument(file.source, file.path, { sourceType: 'script' })),
    };
    istanbulInstrumenter.instrumentSync(file.source, file.path);
    const istanbulFile = istanbulInstrumenter.lastFileCoverage();
    vitestInstrumenter.instrumentSync(file.source, file.path);
    const vitestFile = vitestInstrumenter.lastFileCoverage();
    const oxcFile = JSON.parse(
      oxc.instrument(file.source, file.path, { sourceType: 'script', compat: 'istanbul' }).coverageMap,
    );
    coverageData[file.path] = istanbulFile;
    rows.instrument.push({
      file: file.label,
      kb,
      ...instrumentTimes,
      same: [istanbulFile, vitestFile, oxcFile].map(counts).join(' · '),
    });

    // Convert real precise coverage of one run of the build.
    const url = pathToFileURL(file.path).href;
    const functions = await captureV8(file.source, url);
    const functionsJson = JSON.stringify(functions);
    rows.v8.push({
      file: file.label,
      kb,
      functions: functions.length,
      v8ToIstanbul: await measure(runs, async () => {
        const converter = v8ToIstanbul(file.path, 0, { source: file.source });
        await converter.load();
        converter.applyCoverage(functions);
        return converter.toIstanbul();
      }),
      astV8ToIstanbul: await measure(runs, () =>
        astV8ToIstanbul({
          ast: parseSync(file.path, file.source).program,
          code: file.source,
          wrapperLength: 0,
          coverage: { url, functions },
        }),
      ),
      oxc: await measure(runs, () => oxc.v8ToIstanbul(file.source, file.path, functionsJson)),
    });
    const lineConverter = v8ToIstanbul(file.path, 0, { source: file.source });
    await lineConverter.load();
    lineConverter.applyCoverage(functions);
    const lineOutput = Object.values(lineConverter.toIstanbul())[0];
    const astOutput = Object.values(
      await astV8ToIstanbul({
        ast: parseSync(file.path, file.source).program,
        code: file.source,
        wrapperLength: 0,
        coverage: { url, functions },
      }),
    )[0];
    const oxcV8Output = JSON.parse(oxc.v8ToIstanbul(file.source, file.path, functionsJson));
    rows.v8.at(-1).output = [lineOutput, astOutput, oxcV8Output].map(counts).join(' · ');

    // Remap coverage of a @babel/core build back to the library source.
    const built = babel.transformSync(file.source, {
      filename: file.path,
      sourceMaps: true,
      configFile: false,
      babelrc: false,
      compact: false,
    });
    const builtPath = `${file.path}.built.js`;
    const remapInstrumenter = istanbulInstrument.createInstrumenter({ esModules: false });
    remapInstrumenter.instrumentSync(built.code, builtPath, built.map);
    remapInput[builtPath] = remapInstrumenter.lastFileCoverage();
  }

  for (const file of files) {
    const builtPath = `${file.path}.built.js`;
    const json = JSON.stringify({ [builtPath]: remapInput[builtPath] });
    rows.remap.push({
      file: file.label,
      kb: (file.source.length / 1024).toFixed(0),
      istanbul: await measure(runs, () =>
        istanbulSourceMaps.createSourceMapStore().transformCoverage(istanbulCoverage.createCoverageMap(JSON.parse(json))),
      ),
      vitest: await measure(runs, () =>
        vitestSourceMaps.createSourceMapStore().transformCoverage(vitestCoverage.createCoverageMap(JSON.parse(json))),
      ),
      oxc: await measure(runs, () => oxc.remapCoverageMap(json)),
    });
    const istanbulRemapped = await istanbulSourceMaps
      .createSourceMapStore()
      .transformCoverage(istanbulCoverage.createCoverageMap(JSON.parse(json)));
    const vitestRemapped = await vitestSourceMaps
      .createSourceMapStore()
      .transformCoverage(vitestCoverage.createCoverageMap(JSON.parse(json)));
    const oxcRemapped = Object.values(JSON.parse(oxc.remapCoverageMap(json)))[0];
    rows.remap.at(-1).same = [
      counts(istanbulRemapped.fileCoverageFor(istanbulRemapped.files()[0]).toJSON()),
      counts(vitestRemapped.fileCoverageFor(vitestRemapped.files()[0]).toJSON()),
      counts(oxcRemapped),
    ].join(' · ');
  }

  // Whole-corpus stages, on the istanbul coverage map of all five files.
  const corpusJson = JSON.stringify(coverageData);
  const summarize = (lib) => () => {
    const map = lib.createCoverageMap(JSON.parse(corpusJson));
    for (const path of map.files()) map.fileCoverageFor(path).toSummary();
    return map.getCoverageSummary();
  };
  const summary = {
    istanbul: await measure(runs, summarize(istanbulCoverage)),
    vitest: await measure(runs, summarize(vitestCoverage)),
    oxc: await measure(runs, () => oxc.summarizeCoverage(corpusJson)),
  };
  const istanbulTotal = summarize(istanbulCoverage)().toJSON();
  const oxcTotal = oxc.summarizeCoverage(corpusJson).total;
  const sameSummary = ['lines', 'statements', 'functions', 'branches'].every(
    (key) => istanbulTotal[key].total === oxcTotal[key].total && istanbulTotal[key].covered === oxcTotal[key].covered,
  );

  const reportDir = (name) => {
    const dir = join(scratch, name);
    rmSync(dir, { recursive: true, force: true });
    mkdirSync(dir, { recursive: true });
    return dir;
  };
  const istanbulStyleReport = (reportLib, coverageLib, reportsLib, format) => () => {
    const context = reportLib.createContext({
      dir: reportDir(`${format}-js`),
      coverageMap: coverageLib.createCoverageMap(JSON.parse(corpusJson)),
    });
    const options = format.startsWith('text') ? { file: `${format}.txt` } : {};
    (reportsLib ?? reportLib).create(ISTANBUL_FORMAT[format] ?? format, options).execute(context);
  };
  const reports = [];
  for (const format of [...TEXT_FORMATS, 'html']) {
    reports.push({
      format,
      istanbul: await measure(runs, istanbulStyleReport(istanbulReport, istanbulCoverage, istanbulReports, format)),
      vitest: await measure(runs, istanbulStyleReport(vitestReport, vitestCoverage, null, format)),
      oxc: await measure(runs, () => {
        const dir = reportDir(`${format}-oxc`);
        if (format === 'html') {
          oxc.writeHtmlReport(corpusJson, dir, { rootDir: ROOT });
        } else {
          writeFileSync(join(dir, `${format}.out`), oxc.createReport(corpusJson, format, { rootDir: ROOT }));
        }
      }),
    });
  }
  rmSync(scratch, { recursive: true, force: true });

  const ms = (value) => (value < 10 ? value.toFixed(2) : value.toFixed(0));
  const speedup = (baseline, value) => `${(baseline / value).toFixed(1)}x`;
  const lines = [];
  lines.push(`Measured on ${os.cpus()[0].model}, ${os.platform()} ${os.release()}, Node.js ${process.version}, ${runs} runs (median, ms), oxc binding \`${oxc.__napiBindingTarget}\`.`);
  lines.push('');
  lines.push('Instrument (`instrumentSync` vs `instrument`):');
  lines.push('');
  lines.push('| File | KB | istanbul-lib-instrument | @vitest/istanbul-lib-instrument | oxc | oxc vs istanbul | oxc vs @vitest | s/f/b: istanbul · @vitest · oxc strict |');
  lines.push('|:--|--:|--:|--:|--:|--:|--:|:--|');
  for (const r of rows.instrument) {
    lines.push(`| ${r.file} | ${r.kb} | ${ms(r.istanbul)} | ${ms(r.vitest)} | ${ms(r.oxc)} | ${speedup(r.istanbul, r.oxc)} | ${speedup(r.vitest, r.oxc)} | ${r.same} |`);
  }
  lines.push('');
  lines.push('V8 to Istanbul (one run of each build, real inspector coverage):');
  lines.push('');
  lines.push('| File | V8 functions | v8-to-istanbul | ast-v8-to-istanbul + oxc-parser | oxc | oxc vs v8-to-istanbul | oxc vs ast-v8-to-istanbul | s/f/b out: v8-to-istanbul · ast · oxc |');
  lines.push('|:--|--:|--:|--:|--:|--:|--:|:--|');
  for (const r of rows.v8) {
    lines.push(`| ${r.file} | ${r.functions} | ${ms(r.v8ToIstanbul)} | ${ms(r.astV8ToIstanbul)} | ${ms(r.oxc)} | ${speedup(r.v8ToIstanbul, r.oxc)} | ${speedup(r.astV8ToIstanbul, r.oxc)} | ${r.output} |`);
  }
  lines.push('');
  lines.push('Source-map remap (coverage of a `@babel/core` build, mapped back to the library source):');
  lines.push('');
  lines.push('| File | istanbul-lib-source-maps | @vitest/istanbul-lib-source-maps | oxc | oxc vs istanbul | oxc vs @vitest | s/f/b out: istanbul · @vitest · oxc |');
  lines.push('|:--|--:|--:|--:|--:|--:|:--|');
  for (const r of rows.remap) {
    lines.push(`| ${r.file} | ${ms(r.istanbul)} | ${ms(r.vitest)} | ${ms(r.oxc)} | ${speedup(r.istanbul, r.oxc)} | ${speedup(r.vitest, r.oxc)} | ${r.same} |`);
  }
  lines.push('');
  lines.push('Summary and reports (coverage map of all five files):');
  lines.push('');
  lines.push('| Step | istanbul | @vitest | oxc | oxc vs istanbul | oxc vs @vitest |');
  lines.push('|:--|--:|--:|--:|--:|--:|');
  lines.push(`| summary${sameSummary ? '' : ' (totals differ)'} | ${ms(summary.istanbul)} | ${ms(summary.vitest)} | ${ms(summary.oxc)} | ${speedup(summary.istanbul, summary.oxc)} | ${speedup(summary.vitest, summary.oxc)} |`);
  for (const r of reports) {
    const label = ISTANBUL_FORMAT[r.format] ? `\`${r.format}\` (Istanbul \`${ISTANBUL_FORMAT[r.format]}\`)` : `\`${r.format}\``;
    lines.push(`| ${label} report | ${ms(r.istanbul)} | ${ms(r.vitest)} | ${ms(r.oxc)} | ${speedup(r.istanbul, r.oxc)} | ${speedup(r.vitest, r.oxc)} |`);
  }
  lines.push('');
  lines.push('Package versions: ' + Object.entries(PACKAGES).map(([name, version]) => `\`${name}@${version}\``).join(', ') + '.');
  const output = `${lines.join('\n')}\n`;
  process.stdout.write(output);
  if (markdown) writeFileSync(markdown, output);
};

await main();
