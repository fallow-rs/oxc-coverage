// Checks that the coverage of the React example points at the .tsx source:
// the file key ends in PriceTag.tsx, every statement sits on a line of that
// file, and the tests hit both arms of the discount conditional.

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
const key = Object.keys(coverage).find((path) => path.endsWith('PriceTag.tsx'));
if (!key) fail(`no entry for PriceTag.tsx. Keys: ${Object.keys(coverage).join(', ')}`);

const file = coverage[key];
const sourceLines = readFileSync(resolve(root, 'src', 'PriceTag.tsx'), 'utf8').split('\n').length;
for (const [id, loc] of Object.entries(file.statementMap)) {
  if (loc.start.line < 1 || loc.end.line > sourceLines) fail(`statement ${id} is outside PriceTag.tsx: ${JSON.stringify(loc)}`);
}

const names = Object.values(file.fnMap).map((fn) => fn.name);
for (const name of ['formatAmount', 'PriceTag']) {
  if (!names.includes(name)) fail(`function ${name} missing from fnMap: ${names.join(', ')}`);
}

const conditionals = Object.entries(file.branchMap).filter(([, branch]) => branch.type === 'cond-expr');
if (conditionals.length === 0) fail('no cond-expr branch for the discount conditionals');
for (const [id] of conditionals) {
  if (!file.b[id].every((hits) => hits > 0)) fail(`branch ${id} has an arm with no hits: ${JSON.stringify(file.b[id])}`);
}

console.log(`OK: ${key} has ${Object.keys(file.statementMap).length} statements, ${names.length} functions, ${conditionals.length} fully covered conditionals.`);
