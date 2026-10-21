// Unit tests for scripts/patch-wasi-emnapi-plugins.mjs.

import { strict as assert } from 'node:assert';

import { patchEmnapiPlugins } from './patch-wasi-emnapi-plugins.mjs';

// The CommonJS loader renames both plugins.
const loader = `const {
  emnapiAsyncWorkPlugin: __emnapiAsyncWorkPlugin,
  emnapiTSFNPlugin: __emnapiTSFNPlugin,
  createOnMessage: __wasmCreateOnMessageForFsProxy,
  instantiateNapiModuleSync: __emnapiInstantiateNapiModuleSync,
} = require('@napi-rs/wasm-runtime')
const { createContext: __emnapiCreateContext } = require('@emnapi/runtime')
`;
assert.equal(
  patchEmnapiPlugins(loader),
  `const {
  createOnMessage: __wasmCreateOnMessageForFsProxy,
  instantiateNapiModuleSync: __emnapiInstantiateNapiModuleSync,
} = require('@napi-rs/wasm-runtime')
const { asyncWork: __emnapiAsyncWorkPlugin, tsfn: __emnapiTSFNPlugin } = require('@emnapi/core/plugins')
const { createContext: __emnapiCreateContext } = require('@emnapi/runtime')
`,
);

// The Node worker requires the runtime inside a block, with the plugin names
// unchanged.
const worker = `try {
  const {
    instantiateNapiModuleSync,
    MessageHandler,
    emnapiAsyncWorkPlugin,
    emnapiTSFNPlugin,
  } = require('@napi-rs/wasm-runtime')
  run()
}
`;
assert.equal(
  patchEmnapiPlugins(worker),
  `try {
  const {
    instantiateNapiModuleSync,
    MessageHandler,
  } = require('@napi-rs/wasm-runtime')
  const { asyncWork: emnapiAsyncWorkPlugin, tsfn: emnapiTSFNPlugin } = require('@emnapi/core/plugins')
  run()
}
`,
);

// The browser loader and worker use ES module imports.
const browser = `import {
  emnapiAsyncWorkPlugin as __emnapiAsyncWorkPlugin,
  emnapiTSFNPlugin as __emnapiTSFNPlugin,
  getDefaultContext as __emnapiGetDefaultContext,
} from '@napi-rs/wasm-runtime'
`;
assert.equal(
  patchEmnapiPlugins(browser),
  `import {
  getDefaultContext as __emnapiGetDefaultContext,
} from '@napi-rs/wasm-runtime'
import { asyncWork as __emnapiAsyncWorkPlugin, tsfn as __emnapiTSFNPlugin } from '@emnapi/core/plugins'
`,
);

// A second run changes nothing.
assert.equal(patchEmnapiPlugins(patchEmnapiPlugins(loader)), patchEmnapiPlugins(loader));

// A file that imports neither the plugins nor the patched form means the napi
// template changed, so the script must stop instead of shipping a stale loader.
assert.throws(() => patchEmnapiPlugins("const x = require('@napi-rs/wasm-runtime')\n"), /plugin import/);

console.log('All emnapi plugin patch tests passed.');
