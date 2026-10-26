#!/usr/bin/env node
// Run the Vitest suites of real open-source projects with and without the
// oxc instrumenter, and compare time and coverage totals.
//
// For each project in projects.json the script clones the pinned commit into
// .bench-tmp/vitest-projects, installs it with its own package manager, adds
// the coverage providers and `oxc-coverage-instrument`, and runs
// `vitest run` in four modes:
//
//   none      no coverage, the baseline
//   istanbul  the Istanbul provider with Vitest's default instrumenter
//   oxc       the Istanbul provider with createOxcInstrumenter({ compat: 'istanbul' })
//   v8        the V8 provider, for reference
//
// Each mode runs --runs times (default 3) in rotating order; the report gives
// the median wall time and the coverage overhead against `none`. The Istanbul
// and oxc runs must give the same coverage totals.
//
// Usage:
//   node examples/vitest-real-projects/run.mjs [--runs=3] [--only=hono] [--oxc=0.18.0] [--markdown=<file>]

import { execFileSync, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import { dirname, join } from 'node:path';
import { performance } from 'node:perf_hooks';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, '..', '..');
const WORK_DIR = join(ROOT, '.bench-tmp', 'vitest-projects');
const BENCH_CONFIG = 'vitest.oxc-bench.config.mts';
const MODES = ['none', 'istanbul', 'oxc', 'v8'];
const METRICS = ['lines', 'statements', 'functions', 'branches'];

const parseArgs = () => {
  const args = { runs: 3, only: null, oxc: '0.18.0', markdown: null };
  for (const arg of process.argv.slice(2)) {
    const [key, value] = arg.replace(/^--/, '').split('=');
    if (key === 'runs') args.runs = Number(value);
    else if (key === 'only') args.only = value.split(',');
    else if (key === 'oxc') args.oxc = value;
    else if (key === 'markdown') args.markdown = value;
    else throw new Error(`unknown argument: ${arg}`);
  }
  if (!Number.isInteger(args.runs) || args.runs < 1) throw new Error('--runs must be a positive integer');
  return args;
};

const run = (cwd, command, args, options = {}) =>
  execFileSync(command, args, { cwd, stdio: options.quiet ? 'pipe' : 'inherit', encoding: 'utf8', ...options });

// `corepack` runs the package manager version that the project pins.
const pm = (project) => (project.packageManager === 'yarn' ? ['corepack', ['yarn']] : ['corepack', ['pnpm']]);

const pmRun = (project, cwd, args, options) => {
  const [command, prefix] = pm(project);
  return run(cwd, command, [...prefix, ...args], options);
};

const vitestVersion = (project, cwd) => {
  const out = project.packageManager === 'yarn'
    ? pmRun(project, cwd, ['vitest', '--version'], { quiet: true })
    : pmRun(project, cwd, ['exec', 'vitest', '--version'], { quiet: true });
  return out.match(/vitest\/(\d+\.\d+\.\d+)/)[1];
};

const prepare = (project, oxcVersion) => {
  const cwd = join(WORK_DIR, project.name);
  if (!existsSync(cwd)) {
    run(WORK_DIR, 'git', ['clone', '--quiet', '--filter=blob:none', project.repository, cwd]);
  }
  // A checkout resets package.json, which drops the packages added below, so
  // check out only when the commit differs.
  const head = run(cwd, 'git', ['rev-parse', 'HEAD'], { quiet: true }).trim();
  if (head !== project.commit) run(cwd, 'git', ['checkout', '--quiet', '--force', project.commit]);
  const stamp = join(cwd, '.oxc-bench-installed');
  const wanted = `${project.commit} ${oxcVersion}`;
  const installed = readFileSync(join(cwd, 'package.json'), 'utf8').includes(`"oxc-coverage-instrument"`);
  if (installed && existsSync(stamp) && readFileSync(stamp, 'utf8') === wanted) return cwd;

  if (project.packageManager === 'yarn') {
    pmRun(project, cwd, ['install']);
  } else {
    pmRun(project, cwd, ['install', '--no-frozen-lockfile']);
  }
  const version = vitestVersion(project, cwd);
  const extra = [
    `@vitest/coverage-istanbul@${version}`,
    `@vitest/coverage-v8@${version}`,
    `oxc-coverage-instrument@${oxcVersion}`,
  ];
  if (project.packageManager === 'yarn') {
    pmRun(project, cwd, ['add', '--dev', ...extra]);
  } else {
    const workspaceRoot = existsSync(join(cwd, 'pnpm-workspace.yaml')) ? ['-w'] : [];
    pmRun(project, cwd, ['add', '-D', ...workspaceRoot, ...extra]);
  }
  // Browser-mode projects need the browser binaries of their Playwright version.
  // playwright-core does not export its cli.js, so take the path from the
  // pnpm store directly.
  for (const browser of project.playwrightBrowsers ?? []) {
    const store = join(cwd, 'node_modules', '.pnpm');
    const dir = readdirSync(store).find((entry) => entry.startsWith('playwright-core@'));
    if (!dir) throw new Error(`${project.name}: playwright-core is not installed`);
    // On a fresh Linux machine the browser also needs system libraries.
    const withDeps = process.env.OXC_BENCH_PLAYWRIGHT_WITH_DEPS === '1' ? ['--with-deps'] : [];
    run(cwd, process.execPath, [join(store, dir, 'node_modules', 'playwright-core', 'cli.js'), 'install', ...withDeps, browser]);
  }
  writeFileSync(stamp, wanted);
  return cwd;
};

// Merge the bench coverage settings into the project's own Vitest config, so
// its include, exclude, and project setup stay as they are.
const writeBenchConfig = (project, cwd) => {
  writeFileSync(
    join(cwd, BENCH_CONFIG),
    `import { appendFileSync, mkdirSync } from 'node:fs';
import { performance } from 'node:perf_hooks';
import { IstanbulCoverageProvider } from '@vitest/coverage-istanbul/dist/provider.js';
import { defineConfig, mergeConfig } from 'vitest/config';
import { createOxcInstrumenter } from 'oxc-coverage-instrument/vitest';
import base from './${project.vitestConfig}';

const mode = process.env.OXC_BENCH_MODE ?? 'none';

// Time the provider's transform hook, which runs the instrumenter for every
// file, in both Istanbul modes. Vitest imports this same module, so the
// patched prototype is the one it uses.
let transformMs = 0;
let transformedFiles = 0;
const onFileTransform = IstanbulCoverageProvider.prototype.onFileTransform;
IstanbulCoverageProvider.prototype.onFileTransform = function (...args) {
  const start = performance.now();
  const result = onFileTransform.apply(this, args);
  transformMs += performance.now() - start;
  if (result) transformedFiles += 1;
  return result;
};

// Count the calls into the oxc instrumenter, so a run that silently fell back
// to the default instrumenter is visible.
let oxcCalls = 0;
const countingOxcInstrumenter = (options) => {
  const instrumenter = createOxcInstrumenter({ ...options, compat: 'istanbul' });
  const instrumentSync = instrumenter.instrumentSync.bind(instrumenter);
  instrumenter.instrumentSync = (...args) => {
    oxcCalls += 1;
    return instrumentSync(...args);
  };
  return instrumenter;
};

// Vitest can load this config in more than one process, so each process
// appends its own line.
process.on('exit', () => {
  if (transformedFiles === 0 && oxcCalls === 0) return;
  mkdirSync('./.oxc-bench', { recursive: true });
  appendFileSync('./.oxc-bench/stats.txt', [process.pid, transformMs, transformedFiles, oxcCalls].join(' ') + '\\n');
});
const coverage =
  mode === 'none'
    ? { enabled: false }
    : {
        enabled: true,
        provider: mode === 'v8' ? 'v8' : 'istanbul',
        ...(mode === 'oxc'
          ? { instrumenter: countingOxcInstrumenter }
          : {}),
        reporter: ['json-summary'],
        reportsDirectory: './.oxc-bench/' + mode,
        reportOnFailure: true,
        clean: true,
      };

export default defineConfig(async (env) => {
  const resolved = typeof base === 'function' ? await base(env) : await base;
  return mergeConfig(resolved, { test: { coverage, watch: false } });
});
`,
  );
};

const stripAnsi = (text) => text.replace(/\u001b\[[0-9;]*m/g, '');

// Vitest 5 caches transformed modules on disk (`fsModuleCache`), including
// the instrumented code. Every run starts cold, as a CI run does; a warm
// rerun skips the instrumenter.
const CACHE_DIRS = ['node_modules/.vitest-cache', 'node_modules/.vite', 'node_modules/.vitest', '.vitest-cache'];
const clearCaches = (cwd) => {
  for (const dir of CACHE_DIRS) rmSync(join(cwd, dir), { recursive: true, force: true });
};

// Sum the lines that each Vitest process appended to .oxc-bench/stats.txt.
const readStats = (cwd) => {
  const path = join(cwd, '.oxc-bench', 'stats.txt');
  const stats = { transformMs: 0, transformedFiles: 0, oxcCalls: 0 };
  if (!existsSync(path)) return stats;
  for (const line of readFileSync(path, 'utf8').split('\n').filter(Boolean)) {
    const [, ms, files, calls] = line.split(' ').map(Number);
    stats.transformMs += ms;
    stats.transformedFiles += files;
    stats.oxcCalls += calls;
  }
  return stats;
};

const runVitest = (project, cwd, mode) => {
  clearCaches(cwd);
  rmSync(join(cwd, '.oxc-bench', 'stats.txt'), { force: true });
  const [command, prefix] = pm(project);
  const exec = project.packageManager === 'yarn' ? ['vitest'] : ['exec', 'vitest'];
  const args = [...prefix, ...exec, 'run', '--config', BENCH_CONFIG, ...project.vitestArgs];
  const start = performance.now();
  const result = spawnSync(command, args, {
    cwd,
    encoding: 'utf8',
    env: { ...process.env, OXC_BENCH_MODE: mode, CI: '1', FORCE_COLOR: '0' },
    maxBuffer: 256 * 1024 * 1024,
  });
  const wall = (performance.now() - start) / 1000;
  const output = stripAnsi(`${result.stdout}\n${result.stderr}`);
  const files = output.match(/Test Files\s+(?:\d+ failed \| )?(\d+) passed/);
  const failed = output.match(/Test Files\s+(\d+) failed/);
  return {
    wall,
    ...readStats(cwd),
    testFiles: files ? Number(files[1]) : null,
    failedFiles: failed ? Number(failed[1]) : 0,
    status: result.status,
    tail: output.split('\n').slice(-25).join('\n'),
  };
};

const readTotals = (cwd, mode) => {
  const path = join(cwd, '.oxc-bench', mode, 'coverage-summary.json');
  if (!existsSync(path)) return null;
  const { total } = JSON.parse(readFileSync(path, 'utf8'));
  return Object.fromEntries(METRICS.map((m) => [m, `${total[m].covered}/${total[m].total}`]));
};

const median = (values) => {
  const sorted = [...values].sort((a, b) => a - b);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
};

const main = () => {
  const args = parseArgs();
  const { projects } = JSON.parse(readFileSync(join(HERE, 'projects.json'), 'utf8'));
  mkdirSync(WORK_DIR, { recursive: true });
  const results = [];

  for (const project of projects.filter((p) => !args.only || args.only.includes(p.name))) {
    process.stderr.write(`\n== ${project.name}\n`);
    const cwd = prepare(project, args.oxc);
    writeBenchConfig(project, cwd);
    rmSync(join(cwd, '.oxc-bench'), { recursive: true, force: true });
    const version = vitestVersion(project, cwd);
    const samples = Object.fromEntries(MODES.map((mode) => [mode, []]));
    let last = {};
    for (let round = 0; round < args.runs; round++) {
      // Rotate the order so that a warm disk cache does not favour one mode.
      const order = MODES.map((_, i) => MODES[(i + round) % MODES.length]);
      for (const mode of order) {
        const sample = runVitest(project, cwd, mode);
        process.stderr.write(`  ${mode.padEnd(8)} ${sample.wall.toFixed(1)}s (exit ${sample.status})\n`);
        if (sample.testFiles === null) process.stderr.write(sample.tail + '\n');
        samples[mode].push(sample);
        last[mode] = sample;
      }
    }
    const pick = (mode, key) => samples[mode].map((sample) => sample[key]);
    results.push({
      project,
      version,
      wall: Object.fromEntries(
        MODES.map((mode) => [mode, { median: median(pick(mode, 'wall')), min: Math.min(...pick(mode, 'wall')), max: Math.max(...pick(mode, 'wall')) }]),
      ),
      transformMs: { istanbul: median(pick('istanbul', 'transformMs')), oxc: median(pick('oxc', 'transformMs')) },
      transformedFiles: median(pick('oxc', 'transformedFiles')),
      oxcCalls: Math.min(...pick('oxc', 'oxcCalls')),
      testFiles: last.none.testFiles,
      failedFiles: Object.fromEntries(MODES.map((mode) => [mode, Math.max(...pick(mode, 'failedFiles'))])),
      totals: { istanbul: readTotals(cwd, 'istanbul'), oxc: readTotals(cwd, 'oxc'), v8: readTotals(cwd, 'v8') },
    });
  }

  const sec = (value) => `${value.toFixed(1)} s`;
  const range = (w) => `${sec(w.median)} (${w.min.toFixed(1)} to ${w.max.toFixed(1)})`;
  const lines = [];
  lines.push(
    `Measured on ${os.cpus()[0].model} (${os.cpus().length} cores), ${os.platform()} ${os.release()}, Node.js ${process.version}, \`oxc-coverage-instrument@${args.oxc}\`, ${args.runs} cold runs per mode (Vitest caches cleared before each run).`,
  );
  lines.push('');
  lines.push('Time spent in coverage instrumentation (the Istanbul provider\'s transform hook, summed over all files, median):');
  lines.push('');
  lines.push('| Project | Vitest | Files instrumented | Istanbul (default) | Istanbul + oxc | Speedup |');
  lines.push('|:--|:--|--:|--:|--:|--:|');
  for (const r of results) {
    lines.push(
      `| [${r.project.name}](${r.project.repository}/tree/${r.project.commit}) | ${r.version} | ${r.transformedFiles} | ${sec(r.transformMs.istanbul / 1000)} | ${sec(r.transformMs.oxc / 1000)} | ${(r.transformMs.istanbul / r.transformMs.oxc).toFixed(1)}x |`,
    );
  }
  lines.push('');
  lines.push('Wall time of `vitest run` (median, with the lowest and highest run):');
  lines.push('');
  lines.push('| Project | Test files | No coverage | Istanbul (default) | Istanbul + oxc | V8 |');
  lines.push('|:--|--:|--:|--:|--:|--:|');
  for (const r of results) {
    lines.push(`| ${r.project.name} | ${r.testFiles ?? '?'} | ${range(r.wall.none)} | ${range(r.wall.istanbul)} | ${range(r.wall.oxc)} | ${range(r.wall.v8)} |`);
  }
  lines.push('');
  lines.push('Coverage totals (covered/total). The Istanbul and oxc columns must be equal:');
  lines.push('');
  lines.push('| Project | Metric | Istanbul (default) | Istanbul + oxc | V8 | Equal |');
  lines.push('|:--|:--|--:|--:|--:|:--|');
  let mismatch = false;
  for (const r of results) {
    for (const metric of METRICS) {
      const a = r.totals.istanbul?.[metric] ?? '?';
      const b = r.totals.oxc?.[metric] ?? '?';
      const equal = a === b && a !== '?';
      if (!equal) mismatch = true;
      lines.push(`| ${r.project.name} | ${metric} | ${a} | ${b} | ${r.totals.v8?.[metric] ?? '?'} | ${equal ? 'yes' : 'NO'} |`);
    }
  }
  for (const r of results) {
    const failed = MODES.filter((mode) => r.failedFiles[mode] > 0);
    if (failed.length) {
      lines.push('');
      lines.push(`${r.project.name}: test files failed in mode ${failed.map((mode) => `${mode} (${r.failedFiles[mode]})`).join(', ')}.`);
    }
    if (r.project.note) {
      lines.push('');
      lines.push(`${r.project.name}: ${r.project.note}`);
    }
  }
  const output = `${lines.join('\n')}\n`;
  process.stdout.write(output);
  if (args.markdown) writeFileSync(args.markdown, output);
  if (mismatch || results.some((r) => r.oxcCalls === 0)) process.exitCode = 1;
};

main();
