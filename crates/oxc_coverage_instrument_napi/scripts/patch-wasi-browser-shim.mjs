#!/usr/bin/env node
// Post-build patch for `coverage-instrument.wasi-browser.js`.
//
// `napi build --target wasm32-wasip1-threads` emits a browser shim that
// allocates `new WebAssembly.Memory({ ..., shared: true })` at module top
// level. Without cross-origin isolation (COOP `same-origin` + COEP
// `require-corp`) the host page lacks `SharedArrayBuffer` and the shim
// throws an opaque `TypeError` from inside napi-rs runtime code with no
// hint that the fix lives on the host page rather than in the bundler or
// the package.
//
// This script injects a guard at the top of the generated shim so the
// failure surfaces a precise diagnostic. The shim is regenerated on every
// `napi build`; this script must run after each build (wired into
// `package.json` `scripts.build` and the release workflow).
//
// Idempotent: the guard carries a sentinel comment and re-runs become
// no-ops once the sentinel is present. Safe to invoke when the shim file
// is absent (non-wasm builds): exits 0 with a notice.
//
// Tracks issue #89.

import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const SENTINEL = '// oxc-coverage-instrument: SharedArrayBuffer guard (issue #89)';
const GUARD = `${SENTINEL}
if (typeof SharedArrayBuffer === 'undefined') {
  throw new Error(
    'oxc-coverage-instrument: the browser WASM binding requires SharedArrayBuffer. ' +
      'Enable Cross-Origin-Opener-Policy: same-origin and ' +
      'Cross-Origin-Embedder-Policy: require-corp on your host page so the ' +
      'browser is cross-origin isolated. See ' +
      'https://github.com/fallow-rs/oxc-coverage#runtime-matrix',
  );
}
`;

const __dirname = dirname(fileURLToPath(import.meta.url));
const shimPath = join(__dirname, '..', 'coverage-instrument.wasi-browser.js');

if (!existsSync(shimPath)) {
  console.log(
    `[patch-wasi-browser-shim] ${shimPath} not present; skipping (non-wasm build).`,
  );
  process.exit(0);
}

const original = readFileSync(shimPath, 'utf8');

if (original.includes(SENTINEL)) {
  console.log('[patch-wasi-browser-shim] guard already present; nothing to do.');
  process.exit(0);
}

// napi-rs emits a single import block at the top, then a blank line, then
// the WASI / memory allocation. Inject after the last `import ... from '...'`
// statement so the guard runs before any `WebAssembly.Memory({ shared: true })`.
const importRegex = /(^import[\s\S]*?from\s+['"][^'"]+['"];?\s*\n)+/m;
const match = original.match(importRegex);
if (!match) {
  console.error(
    '[patch-wasi-browser-shim] could not locate napi-rs import block in shim; aborting.',
  );
  process.exit(1);
}
const insertAt = match.index + match[0].length;
const patched = `${original.slice(0, insertAt)}\n${GUARD}\n${original.slice(insertAt)}`;

writeFileSync(shimPath, patched);
console.log(`[patch-wasi-browser-shim] injected SharedArrayBuffer guard into ${shimPath}.`);
