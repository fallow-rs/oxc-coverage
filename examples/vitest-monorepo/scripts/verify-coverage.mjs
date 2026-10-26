// Checks that one coverage report holds the files of both projects, keyed by
// their .ts sources, and that the class methods of the api package are named.

import { existsSync, readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const coveragePath = resolve(root, 'coverage', 'coverage-final.json');

const fail = (message) => {
  console.error(`FAIL: ${message}`);
  process.exit(1);
};

if (!existsSync(coveragePath)) fail(`${coveragePath} not found. Run \`npm run coverage\` first.`);
const coverage = JSON.parse(readFileSync(coveragePath, 'utf8'));
const keys = Object.keys(coverage);
const slug = keys.find((path) => path.endsWith('packages/core/src/slug.ts'));
const routes = keys.find((path) => path.endsWith('packages/api/src/routes.ts'));
if (!slug || !routes) fail(`expected slug.ts and routes.ts in one report. Keys: ${keys.join(', ')}`);

const names = Object.values(coverage[routes].fnMap).map((fn) => fn.name);
for (const name of ['create', 'find']) {
  if (!names.includes(name)) fail(`method ${name} missing from the fnMap of routes.ts: ${names.join(', ')}`);
}
const loop = Object.values(coverage[slug].s).some((hits) => hits > 1);
if (!loop) fail('the while loop in uniqueSlug was not counted');

console.log(`OK: one report with ${keys.length} files from both projects.`);
