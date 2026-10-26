// Turns coverage/coverage-final.json into the reports a CI system reads:
// lcov.info (Codecov, Coveralls, SonarQube), cobertura-coverage.xml (GitLab,
// Azure DevOps, Jenkins), a text summary, and an HTML report. It then fails
// the run when a metric is under its threshold.
//
// Usage: node scripts/report.mjs [--min-lines=80] [--min-branches=80]

import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createReport, summarizeCoverage, writeHtmlReport } from 'oxc-coverage-instrument';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const outDir = resolve(root, 'coverage');
const json = readFileSync(resolve(outDir, 'coverage-final.json'), 'utf8');

const thresholds = { lines: 80, statements: 0, functions: 0, branches: 0 };
for (const arg of process.argv.slice(2)) {
  const match = arg.match(/^--min-(lines|statements|functions|branches)=(\d+(?:\.\d+)?)$/);
  if (!match) throw new Error(`unknown argument: ${arg}`);
  thresholds[match[1]] = Number(match[2]);
}

// rootDir makes the paths in lcov and cobertura relative to the repository.
mkdirSync(outDir, { recursive: true });
writeFileSync(resolve(outDir, 'lcov.info'), createReport(json, 'lcov', { rootDir: root }));
writeFileSync(resolve(outDir, 'cobertura-coverage.xml'), createReport(json, 'cobertura', { rootDir: root }));
writeHtmlReport(json, resolve(outDir, 'html'), { rootDir: root });
process.stdout.write(createReport(json, 'text-summary'));

const { total } = summarizeCoverage(json);
const failures = Object.entries(thresholds).filter(([metric, min]) => total[metric].pct < min);
for (const [metric, min] of failures) {
  console.error(`Coverage for ${metric} (${total[metric].pct}%) is under the threshold (${min}%).`);
}
if (process.env.GITHUB_STEP_SUMMARY) {
  const rows = ['lines', 'statements', 'functions', 'branches']
    .map((metric) => `| ${metric} | ${total[metric].covered}/${total[metric].total} | ${total[metric].pct}% |`)
    .join('\n');
  writeFileSync(process.env.GITHUB_STEP_SUMMARY, `| Metric | Covered | % |\n|:--|--:|--:|\n${rows}\n`, { flag: 'a' });
}
process.exitCode = failures.length ? 1 : 0;
