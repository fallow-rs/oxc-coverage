#!/usr/bin/env node
// Conformance gates for the strict Istanbul profile and the default Oxc profile.
//
// The strict gate compares every contracted coverage field with
// istanbul-lib-instrument. The default gate compares against that strict shape
// and permits only the extensions documented in the instrumenter README.
//
// Usage:
//   node scripts/istanbul-diff.mjs
//   node scripts/istanbul-diff.mjs --profile=strict
//   node scripts/istanbul-diff.mjs --profile=default

import { createInstrumenter } from 'istanbul-lib-instrument';
import { readdirSync, readFileSync } from 'fs';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';
import { createOxcInstrumenter } from '../crates/oxc_coverage_instrument_napi/vitest.js';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const fixturesDir = join(
  scriptDir,
  '..',
  'crates',
  'oxc_coverage_instrument',
  'tests',
  'conformance',
  'fixtures'
);

const requestedProfile = process.argv.find((arg) => arg.startsWith('--profile='))?.split('=')[1];
if (requestedProfile !== undefined && requestedProfile !== 'strict' && requestedProfile !== 'default') {
  console.error(`Unknown profile: ${requestedProfile}`);
  process.exit(2);
}

const istanbulJs = createInstrumenter({ esModules: true, produceSourceMap: false });
const istanbulTs = createInstrumenter({
  esModules: true,
  produceSourceMap: false,
  parserPlugins: ['typescript'],
});

const normalizeFn = (entry) => ({
  name: entry.name,
  line: entry.line,
  decl: entry.decl,
  loc: entry.loc,
});

const normalizeBranch = (entry) => ({
  type: entry.type,
  line: entry.line,
  loc: entry.loc,
  locations: entry.locations,
});

const normalize = (coverage) => ({
  path: coverage.path,
  statementMap: coverage.statementMap,
  fnMap: Object.fromEntries(
    Object.entries(coverage.fnMap).map(([id, entry]) => [id, normalizeFn(entry)])
  ),
  branchMap: Object.fromEntries(
    Object.entries(coverage.branchMap).map(([id, entry]) => [id, normalizeBranch(entry)])
  ),
  s: coverage.s,
  f: coverage.f,
  b: coverage.b,
});

const isEqual = (left, right) => JSON.stringify(left) === JSON.stringify(right);

const comparePositions = (left, right) => left.line - right.line || left.column - right.column;

const diffKeys = (left, right, path = '') => {
  if (isEqual(left, right)) return [];
  if (
    typeof left !== 'object' ||
    typeof right !== 'object' ||
    left === null ||
    right === null
  ) {
    return [{ path, expected: left, actual: right }];
  }

  const diffs = [];
  const keys = new Set([...Object.keys(left), ...Object.keys(right)]);
  for (const key of keys) {
    diffs.push(...diffKeys(left[key], right[key], path ? `${path}.${key}` : key));
  }
  return diffs;
};

const isAnonymousName = (name) => /^\(anonymous_\d+\)$/.test(name);

const isEmptyPosition = (position) =>
  position !== null && typeof position === 'object' && Object.keys(position).length === 0;

const isEmptyLocation = (location) =>
  isEmptyPosition(location?.start) && isEmptyPosition(location?.end);

const isRealZeroWidthLocation = (location) =>
  Number.isInteger(location?.start?.line) &&
  Number.isInteger(location?.start?.column) &&
  isEqual(location.start, location.end);

const locationText = (source, location) => {
  const lines = source.split('\n');
  const startLine = location?.start?.line;
  const endLine = location?.end?.line;
  const startColumn = location?.start?.column;
  const endColumn = location?.end?.column;
  if (
    !Number.isInteger(startLine) ||
    !Number.isInteger(endLine) ||
    !Number.isInteger(startColumn) ||
    !Number.isInteger(endColumn) ||
    startLine < 1 ||
    endLine < startLine ||
    endLine > lines.length
  ) {
    return '';
  }
  if (startLine === endLine) return lines[startLine - 1].slice(startColumn, endColumn);
  return [
    lines[startLine - 1].slice(startColumn),
    ...lines.slice(startLine, endLine - 1),
    lines[endLine - 1].slice(0, endColumn),
  ].join('\n');
};

const compareFunctionMaps = (strict, defaults, observedExtensions) => {
  const diffs = [];
  const strictIds = Object.keys(strict.fnMap);
  if (!isEqual(strictIds, Object.keys(defaults.fnMap))) {
    return diffKeys(strictIds, Object.keys(defaults.fnMap), 'fnMap.keys');
  }

  for (const id of strictIds) {
    const expected = strict.fnMap[id];
    const actual = defaults.fnMap[id];
    diffs.push(...diffKeys(expected.line, actual.line, `fnMap.${id}.line`));
    diffs.push(...diffKeys(expected.loc, actual.loc, `fnMap.${id}.loc`));

    const inferredName =
      isAnonymousName(expected.name) &&
      typeof actual.name === 'string' &&
      actual.name.length > 0 &&
      !isAnonymousName(actual.name);
    if (expected.name !== actual.name && !inferredName) {
      diffs.push({ path: `fnMap.${id}.name`, expected: expected.name, actual: actual.name });
    } else if (expected.name !== actual.name) {
      observedExtensions.add('inferred function names');
    }

    if (!isEqual(expected.decl, actual.decl)) {
      // The strict marker sits on the first character of the method, which can
      // be a decorator or a modifier; the default span is the key, which comes
      // later but always ends before the body.
      const widenedMethodSpan =
        inferredName &&
        expected.decl.start.line === expected.decl.end.line &&
        expected.decl.end.column === expected.decl.start.column + 1 &&
        actual.decl.start.line === actual.decl.end.line &&
        comparePositions(actual.decl.start, expected.decl.start) >= 0 &&
        comparePositions(actual.decl.end, actual.decl.start) > 0 &&
        comparePositions(actual.decl.end, actual.loc.start) <= 0;
      if (!widenedMethodSpan) {
        diffs.push({ path: `fnMap.${id}.decl`, expected: expected.decl, actual: actual.decl });
      } else {
        observedExtensions.add('full method spans');
      }
    }
  }
  return diffs;
};

const branchMatches = (strict, defaults) => {
  if (
    strict.type !== defaults.type ||
    strict.line !== defaults.line ||
    !isEqual(strict.loc, defaults.loc) ||
    strict.locations.length !== defaults.locations.length
  ) {
    return false;
  }
  return strict.locations.every((location, index) => {
    const candidate = defaults.locations[index];
    return isEqual(location, candidate) ||
      (isEmptyLocation(location) && isRealZeroWidthLocation(candidate));
  });
};

const isDocumentedExtraBranch = (source, entry) => {
  if (!Array.isArray(entry.locations) || entry.locations.length !== 2) return false;
  const coveredSource = locationText(source, entry.loc);
  if (entry.type === 'optional-chain') return coveredSource.includes('?.');
  return entry.type === 'binary-expr' && /(?:\?\?=|\|\|=|&&=)/.test(coveredSource);
};

const compareBranchMaps = (strict, defaults, source, observedExtensions) => {
  const diffs = [];
  const availableDefaultIds = new Set(Object.keys(defaults.branchMap));

  for (const [strictId, expected] of Object.entries(strict.branchMap)) {
    const defaultId = [...availableDefaultIds].find((id) =>
      branchMatches(expected, defaults.branchMap[id])
    );
    if (defaultId === undefined) {
      diffs.push({ path: `branchMap.${strictId}`, expected, actual: undefined });
      continue;
    }
    availableDefaultIds.delete(defaultId);
    if (
      expected.locations.some(
        (location, index) =>
          isEmptyLocation(location) &&
          isRealZeroWidthLocation(defaults.branchMap[defaultId].locations[index])
      )
    ) {
      observedExtensions.add('real synthetic else locations');
    }
    diffs.push(...diffKeys(strict.b[strictId], defaults.b[defaultId], `b.${strictId}`));
  }

  for (const defaultId of availableDefaultIds) {
    const entry = defaults.branchMap[defaultId];
    const counters = defaults.b[defaultId];
    const validCounters =
      Array.isArray(counters) &&
      counters.length === entry.locations.length &&
      counters.every((count) => count === 0);
    if (!isDocumentedExtraBranch(source, entry) || !validCounters) {
      diffs.push({
        path: `branchMap.${defaultId}`,
        expected: 'documented default-profile branch',
        actual: { entry, counters },
      });
    } else if (entry.type === 'optional-chain') {
      observedExtensions.add('optional-chain branches');
    } else {
      observedExtensions.add('logical-assignment branches');
    }
  }
  return diffs;
};

const compareDefaultProfile = (strict, defaults, source, observedExtensions) => [
  ...diffKeys(strict.path, defaults.path, 'path'),
  ...diffKeys(strict.statementMap, defaults.statementMap, 'statementMap'),
  ...diffKeys(strict.s, defaults.s, 's'),
  ...diffKeys(strict.f, defaults.f, 'f'),
  ...compareFunctionMaps(strict, defaults, observedExtensions),
  ...compareBranchMaps(strict, defaults, source, observedExtensions),
];

const fixtures = readdirSync(fixturesDir).filter((file) => /\.(?:js|ts)$/.test(file)).sort();
const cases = fixtures.map((file) => ({
  file,
  source: readFileSync(join(fixturesDir, file), 'utf8'),
}));
cases.push(
  {
    file: 'profile-logical-and-optional.js',
    source: 'let value; value ??= fallback; const nested = object?.property;',
  },
  {
    file: 'profile-names-methods-and-synthetic-else.js',
    source: 'export default function () {} const object = { execute() {} }; if (value) work();',
  }
);

const printDiffs = (profile, file, diffs) => {
  console.log(`[DIFF:${profile}] ${file}`);
  for (const diff of diffs.slice(0, 5)) {
    console.log(
      `  ${diff.path}: expected=${JSON.stringify(diff.expected)} actual=${JSON.stringify(diff.actual)}`
    );
  }
  if (diffs.length > 5) console.log(`  ... and ${diffs.length - 5} more`);
};

const runStrictProfile = () => {
  let failed = false;
  for (const { file, source } of cases) {
    const istanbul = file.endsWith('.ts') ? istanbulTs : istanbulJs;
    const oxc = createOxcInstrumenter({ coverageVariable: '__coverage__', compat: 'istanbul' });
    istanbul.instrumentSync(source, file);
    oxc.instrumentSync(source, file);
    const diffs = diffKeys(
      normalize(istanbul.lastFileCoverage()),
      normalize(oxc.lastFileCoverage())
    );
    if (diffs.length === 0) {
      console.log(`[OK:strict] ${file}`);
    } else {
      failed = true;
      printDiffs('strict', file, diffs);
    }
  }
  console.log(failed
    ? 'FAIL: strict profile differs from istanbul-lib-instrument.'
    : 'PASS: strict profile matches istanbul-lib-instrument without filters.');
  return !failed;
};

const runDefaultProfile = () => {
  let failed = false;
  const observedExtensions = new Set();
  for (const { file, source } of cases) {
    const strict = createOxcInstrumenter({ coverageVariable: '__coverage__', compat: 'istanbul' });
    const defaults = createOxcInstrumenter({ coverageVariable: '__coverage__' });
    strict.instrumentSync(source, file);
    defaults.instrumentSync(source, file);
    const diffs = compareDefaultProfile(
      normalize(strict.lastFileCoverage()),
      normalize(defaults.lastFileCoverage()),
      source,
      observedExtensions
    );
    if (diffs.length === 0) {
      console.log(`[OK:default] ${file}`);
    } else {
      failed = true;
      printDiffs('default', file, diffs);
    }
  }
  const requiredExtensions = [
    'inferred function names',
    'full method spans',
    'real synthetic else locations',
    'logical-assignment branches',
    'optional-chain branches',
  ];
  const missingExtensions = requiredExtensions.filter(
    (extension) => !observedExtensions.has(extension)
  );
  if (missingExtensions.length > 0) {
    failed = true;
    printDiffs('default', 'documented extensions', [
      {
        path: 'extensions',
        expected: requiredExtensions,
        actual: [...observedExtensions],
      },
    ]);
  }
  console.log(failed
    ? 'FAIL: default profile contains an undocumented coverage-shape difference.'
    : 'PASS: default profile differs only by documented Oxc extensions.');
  return !failed;
};

let success = true;
if (requestedProfile === undefined || requestedProfile === 'strict') {
  success = runStrictProfile() && success;
}
if (requestedProfile === undefined || requestedProfile === 'default') {
  success = runDefaultProfile() && success;
}
process.exit(success ? 0 : 1);
