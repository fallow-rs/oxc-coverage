// Checks the reports that scripts/report.mjs wrote: lcov names the source by
// a relative path and lists the uncovered function, cobertura is a coverage
// document for the same file, and the HTML report has an index page.

import { existsSync, readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const read = (path) => {
  const full = resolve(root, 'coverage', path);
  if (!existsSync(full)) {
    console.error(`FAIL: ${full} not found. Run \`npm run coverage\` and \`npm run report\` first.`);
    process.exit(1);
  }
  return readFileSync(full, 'utf8');
};
const fail = (message) => {
  console.error(`FAIL: ${message}`);
  process.exit(1);
};

const lcov = read('lcov.info');
if (!lcov.includes('SF:src/invoice.ts')) fail('lcov.info does not name src/invoice.ts by a relative path');
if (!/FNDA:0,formatInvoiceNumber/.test(lcov)) fail('lcov.info does not list formatInvoiceNumber as uncovered');

const cobertura = read('cobertura-coverage.xml');
if (!cobertura.includes('<coverage') || !cobertura.includes('filename="src/invoice.ts"')) {
  fail('cobertura-coverage.xml has no entry for src/invoice.ts');
}

if (!read('html/index.html').includes('invoice.ts')) fail('the HTML index does not list invoice.ts');

console.log('OK: lcov, cobertura, and HTML reports are present and name src/invoice.ts.');
