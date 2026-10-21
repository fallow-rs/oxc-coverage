// Loads the emnapi async-work and threadsafe-function plugins from
// `@emnapi/core/plugins` in the WASI loaders that `napi build` writes.
//
// The napi-rs template imports both plugins from `@napi-rs/wasm-runtime`, which
// bundles its own copy. That copy sets `emnapi_shared.napiModule.initWorker`,
// but emnapi 2.0.0-beta renamed the field to `addonModule`, so the binding
// fails to load with "Cannot set properties of undefined (setting
// 'initWorker')". The plugins that `@emnapi/core` ships match the runtime that
// this package depends on. Drop this script when `@napi-rs/wasm-runtime`
// supports the emnapi 2 beta.
//
// `napi build` rewrites the loaders, so this must run after every wasm build.
// It is safe to run more than once.

import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

// The single-threaded build writes `wasip1` names until
// patch-wasi-singlethreaded-artifacts.mjs renames them, so both forms count.
const LOADERS = [
  'coverage-instrument.wasi.cjs',
  'coverage-instrument.wasi-browser.js',
  'coverage-instrument.wasip1.cjs',
  'coverage-instrument.wasip1-browser.js',
  'wasi-worker.mjs',
  'wasi-worker-browser.mjs',
];

// The plugin names in `@napi-rs/wasm-runtime`, and their names in
// `@emnapi/core/plugins`, in import order.
const PLUGINS = [
  ['emnapiAsyncWorkPlugin', 'asyncWork'],
  ['emnapiTSFNPlugin', 'tsfn'],
];

const RUNTIME_IMPORT = [
  /^([ \t]*)const \{\n([^}]*)\} = require\('@napi-rs\/wasm-runtime'\)\n/m,
  /^([ \t]*)import \{\n([^}]*)\} from '@napi-rs\/wasm-runtime'\n/m,
];
const ENTRY = /^\s*(\w+)(?:\s*(?::|as)\s*(\w+))?,?\s*$/;
const PATCHED = "'@emnapi/core/plugins'";

export function patchEmnapiPlugins(source) {
  for (const [index, pattern] of RUNTIME_IMPORT.entries()) {
    const match = pattern.exec(source);
    if (!match) continue;
    const [statement, indent, body] = match;
    const kept = [];
    const local = new Map();
    for (const line of body.split('\n')) {
      const entry = ENTRY.exec(line);
      const plugin = entry && PLUGINS.find(([name]) => name === entry[1]);
      if (plugin) local.set(plugin[0], entry[2] ?? entry[1]);
      else kept.push(line);
    }
    if (local.size === 0) continue;
    if (local.size !== PLUGINS.length) {
      throw new Error('the napi template changed: it imports only some of the emnapi plugins');
    }
    const esm = index === 1;
    const names = PLUGINS.map(([name, upstream]) =>
      esm ? `${upstream} as ${local.get(name)}` : `${upstream}: ${local.get(name)}`,
    ).join(', ');
    const pluginImport = esm
      ? `${indent}import { ${names} } from ${PATCHED}\n`
      : `${indent}const { ${names} } = require(${PATCHED})\n`;
    const runtime = esm
      ? `${indent}import {\n${kept.join('\n')}} from '@napi-rs/wasm-runtime'\n`
      : `${indent}const {\n${kept.join('\n')}} = require('@napi-rs/wasm-runtime')\n`;
    return source.replace(statement, runtime + pluginImport);
  }
  if (source.includes(PATCHED)) return source;
  throw new Error('the napi template changed: no emnapi plugin import from @napi-rs/wasm-runtime');
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const targetDir = process.argv[2] ?? '.';
  let patched = 0;
  for (const name of LOADERS) {
    const path = join(targetDir, name);
    if (!existsSync(path)) continue;
    const original = readFileSync(path, 'utf8');
    const next = patchEmnapiPlugins(original);
    if (next !== original) {
      writeFileSync(path, next);
      patched += 1;
    }
  }
  console.log(`[patch-wasi-emnapi-plugins] patched ${patched} WASI loader(s) in ${targetDir}.`);
}
