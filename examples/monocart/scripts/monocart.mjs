// Shared helpers: give Istanbul coverage data to monocart-coverage-reports and
// compare its totals with the totals of oxc-coverage-instrument.

import { MCR } from 'monocart-coverage-reports';
import { summarizeCoverage } from 'oxc-coverage-instrument';

export const METRICS = ['statements', 'branches', 'functions', 'lines'];

/**
 * Generates monocart reports from an Istanbul coverage map and returns the
 * monocart summary.
 */
export const generateMonocartReports = async (coverageMap, outputDir, reports) => {
  const mcr = MCR({
    name: 'oxc-coverage-instrument with monocart',
    outputDir,
    reports,
    logging: 'error',
  });
  mcr.cleanCache();
  await mcr.add(coverageMap);
  const results = await mcr.generate();
  if (!results) throw new Error('monocart-coverage-reports returned no results');
  return results.summary;
};

/**
 * Compares the monocart summary with summarizeCoverage() of
 * oxc-coverage-instrument. Returns one row for each metric.
 */
export const compareTotals = (monocartSummary, coverageJson) => {
  const { total } = summarizeCoverage(coverageJson);
  return METRICS.map((metric) => {
    const ours = total[metric];
    const theirs = monocartSummary[metric];
    return {
      metric,
      'oxc-coverage-instrument': `${ours.covered}/${ours.total}`,
      monocart: `${theirs.covered}/${theirs.total}`,
      same: ours.covered === theirs.covered && ours.total === theirs.total,
    };
  });
};

/** Prints the rows and sets exit code 1 when a metric differs. */
export const reportComparison = (label, rows) => {
  console.log(`\n${label}`);
  console.table(rows);
  const different = rows.filter((row) => !row.same);
  if (different.length) {
    console.error(`${label}: totals differ for ${different.map((row) => row.metric).join(', ')}`);
    process.exitCode = 1;
  }
};
