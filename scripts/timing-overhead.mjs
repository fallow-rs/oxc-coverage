#!/usr/bin/env node
// Overhead of the experimental `timing` option on real library workloads.
//
// Each workload runs in a fresh process per variant, so one variant cannot
// warm the JIT for another one:
//
//   plain     the source as published, not instrumented
//   coverage  coverage counters only (the baseline for timing)
//   every-1   coverage + timing, every call timed
//   every-N   coverage + timing, one call in N timed
//
// Usage:
//   node scripts/prepare-real-world-corpus.mjs
//   npm --prefix crates/oxc_coverage_instrument_napi run build:debug
//   node scripts/timing-overhead.mjs [--runtime node|bun] [--runs 11]
//
// The script prints one Markdown table per runtime. A run is the CPU time
// (user + system) of one measured batch after a warm-up batch. CPU time is
// used instead of wall time, because wall time on a shared machine changes
// with the load of other processes. The table gives the median and, after the
// slash, the minimum of all runs.

import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const root = resolve(scriptDir, '..');
const corpusDir = join(root, '.bench-tmp', 'files');
const outDir = join(root, '.bench-tmp', 'timing');

const args = process.argv.slice(2);
const flag = (name, fallback) => {
  const index = args.indexOf(name);
  return index === -1 ? fallback : args[index + 1];
};

if (args[0] === '--child') {
  await runChild(args[1], args[2]);
} else {
  runParent();
}

function runParent() {
  const runtime = flag('--runtime', 'node');
  const runs = Number(flag('--runs', '11'));
  const require = createRequire(import.meta.url);
  const { instrument } = require(join(root, 'crates/oxc_coverage_instrument_napi/index.js'));
  mkdirSync(outDir, { recursive: true });

  const variants = [
    { name: 'plain', options: null },
    { name: 'coverage', options: {} },
    { name: 'every-1', options: { timing: { sampleEvery: 1 } } },
    { name: 'every-64', options: { timing: { sampleEvery: 64 } } },
    { name: 'every-1024', options: { timing: { sampleEvery: 1024 } } },
  ];
  const libraries = ['lodash.js', 'd3.js', 'three.js'];
  for (const variant of variants) {
    const dir = join(outDir, variant.name);
    mkdirSync(dir, { recursive: true });
    for (const file of [...libraries, 'async-service.js']) {
      const sourcePath = file === 'async-service.js' ? join(scriptDir, 'timing-fixtures', file) : join(corpusDir, file);
      const source = readFileSync(sourcePath, 'utf8');
      const code = variant.options === null ? source : instrument(source, file, variant.options).code;
      // `.cjs`: the root manifest declares ES modules, and the UMD bundles are
      // CommonJS.
      writeFileSync(join(dir, file.replace(/\.js$/, '.cjs')), code);
    }
  }

  const workloads = ['lodash', 'd3', 'three', 'async-service'];
  const rows = [];
  for (const workload of workloads) {
    const medians = {};
    const minimums = {};
    const sampled = {};
    for (const variant of variants) {
      const times = [];
      let calls = 0;
      for (let run = 0; run < runs; run++) {
        const child = spawnSync(runtime, [fileURLToPath(import.meta.url), '--child', variant.name, workload], {
          encoding: 'utf8',
        });
        if (child.status !== 0) {
          throw new Error(`${runtime} ${variant.name} ${workload} failed:\n${child.stderr}`);
        }
        const result = JSON.parse(child.stdout.trim().split('\n').at(-1));
        times.push(result.cpuMs);
        calls = result.sampledCalls;
      }
      times.sort((a, b) => a - b);
      medians[variant.name] = times[Math.floor(times.length / 2)];
      minimums[variant.name] = times[0];
      sampled[variant.name] = calls;
    }
    rows.push({ workload, medians, minimums, sampled });
  }

  const pct = (value, base) => `${(((value - base) / base) * 100).toFixed(1)}%`;
  const version = spawnSync(runtime, ['--version'], { encoding: 'utf8' }).stdout.trim();
  console.log(`\n${runtime} ${version}, ${runs} runs, CPU ms of one batch, median / minimum\n`);
  const names = variants.map((variant) => variant.name);
  console.log(`| workload | ${names.join(' | ')} | ${names.slice(2).map((name) => `${name} vs coverage`).join(' | ')} | timed calls every-1 / every-64 |`);
  console.log(`|:--|${names.map(() => '--:').join('|')}|${names.slice(2).map(() => '--:').join('|')}|--:|`);
  for (const { workload, medians: m, minimums: n, sampled } of rows) {
    const cells = names.map((name) => `${m[name].toFixed(1)} / ${n[name].toFixed(1)}`);
    const deltas = names.slice(2).map((name) => `${pct(m[name], m.coverage)} / ${pct(n[name], n.coverage)}`);
    console.log(`| ${workload} | ${cells.join(' | ')} | ${deltas.join(' | ')} | ${sampled['every-1']} / ${sampled['every-64']} |`);
  }
}

async function runChild(variant, workload) {
  const require = createRequire(import.meta.url);
  const dir = join(outDir, variant);
  const load = (file) => require(join(dir, file.replace(/\.js$/, '.cjs')));
  const job = await workloadFor(workload, load);
  await job(); // warm-up batch
  const start = performance.now();
  const cpuStart = process.cpuUsage();
  await job();
  const cpu = process.cpuUsage(cpuStart);
  const ms = performance.now() - start;
  const cpuMs = (cpu.user + cpu.system) / 1000;
  const timing = globalThis.__fallow_timing__ ?? {};
  let sampledCalls = 0;
  for (const entry of Object.values(timing)) {
    for (const count of entry.c) sampledCalls += count;
  }
  console.log(JSON.stringify({ ms, cpuMs, sampledCalls }));
}

async function workloadFor(workload, load) {
  switch (workload) {
    case 'lodash': {
      const _ = load('lodash.js');
      const records = Array.from({ length: 4000 }, (_unused, i) => ({
        id: i,
        name: `user ${i} of group ${i % 37}`,
        group: `g${i % 37}`,
        score: (i * 7919) % 1000,
        tags: [`t${i % 5}`, `t${i % 11}`],
        nested: { a: { b: { c: i } } },
      }));
      const template = _.template('<li><%- name %> (<%= score %>)</li>');
      return () => {
        let checksum = 0;
        for (let round = 0; round < 6; round++) {
          const groups = _.groupBy(records, 'group');
          const sorted = _.sortBy(records, ['score', 'id']);
          const top = _.take(_.orderBy(records, ['score'], ['desc']), 50);
          const tags = _.uniq(_.flatMap(records, 'tags'));
          const merged = _.merge({}, ..._.take(records, 200).map((r) => r.nested));
          const cloned = _.cloneDeep(_.take(records, 500));
          const names = _.map(records, (r) => _.camelCase(r.name));
          const html = top.map((r) => template(r)).join('');
          const chunks = _.chunk(sorted, 64).map((c) => _.sumBy(c, 'score'));
          checksum +=
            Object.keys(groups).length + tags.length + merged.a.b.c + cloned.length + names.length + html.length + _.sum(chunks);
        }
        return checksum;
      };
    }
    case 'd3': {
      const d3 = load('d3.js');
      const values = Array.from({ length: 20000 }, (_unused, i) => Math.sin(i / 50) * 100 + (i % 97));
      return () => {
        let checksum = 0;
        for (let round = 0; round < 6; round++) {
          const x = d3.scaleLinear().domain(d3.extent(values)).range([0, 960]).nice();
          const y = d3.scaleLog().domain([1, 1000]).range([500, 0]);
          const bins = d3.bin().domain(x.domain()).thresholds(x.ticks(40))(values);
          const line = d3.line().x((_d, i) => i / 20).y((d) => x(d)).curve(d3.curveMonotoneX);
          const path = line(values.slice(0, 5000));
          const format = d3.format(',.2f');
          const labels = bins.map((b) => format(b.length) + format(y(b.length + 1)));
          const stats = [d3.mean(values), d3.median(values), d3.deviation(values), d3.quantile(values, 0.95)];
          const grouped = d3.rollup(values, (v) => v.length, (d) => Math.round(d / 10));
          checksum += path.length + labels.join('').length + stats.reduce((a, b) => a + b, 0) + grouped.size;
        }
        return checksum;
      };
    }
    case 'three': {
      const THREE = load('three.js');
      return () => {
        let checksum = 0;
        const m = new THREE.Matrix4();
        const q = new THREE.Quaternion();
        const v = new THREE.Vector3();
        const box = new THREE.Box3();
        for (let i = 0; i < 300000; i++) {
          q.setFromAxisAngle(new THREE.Vector3(0, 1, 0), i / 1000);
          m.compose(new THREE.Vector3(i % 10, 1, 2), q, new THREE.Vector3(1, 1, 1));
          v.set(i % 7, i % 11, i % 13).applyMatrix4(m).normalize();
          box.expandByPoint(v);
          checksum += v.x + v.y + v.z;
        }
        return checksum + box.max.x;
      };
    }
    case 'async-service': {
      const service = load('async-service.js');
      return () => service.runBatch(3000);
    }
    default:
      throw new Error(`unknown workload ${workload}`);
  }
}
