// Compares the coverage-final.json files of the three karma runs.
//
// - oxc-strict (compat: 'istanbul') must have the same statement, function,
//   and branch maps as istanbul, and the same hit counts. The file objects
//   are not equal as a whole: `hash` is a different digest, and
//   oxc-coverage-instrument does not write `_coverageSchema`. The script
//   checks that no other top-level key differs.
// - oxc (default profile) must have the same statements and the same hit
//   counts. It can differ from istanbul only by the extensions that
//   crates/oxc_coverage_instrument/README.md documents:
//     1. a branch entry for each `??=`, `||=`, `&&=`, and optional chain,
//     2. inferred function names in place of `(anonymous_N)`,
//     3. full method key spans in place of a one-character `decl`,
//     4. a real zero-width position for the implicit `else` of an `if`.
import { readFileSync } from 'node:fs';
import { relative } from 'node:path';
import { isDeepStrictEqual as isEqual } from 'node:util';

const load = (name) => JSON.parse(readFileSync(`coverage/${name}/coverage-final.json`, 'utf8'));

const comparePositions = (left, right) => left.line - right.line || left.column - right.column;
const isEmptyPosition = (position) => isEqual(position, {});
const isEmptyLocation = (location) =>
  isEmptyPosition(location?.start) && isEmptyPosition(location?.end);
const isZeroWidthLocation = (location) =>
  Number.isInteger(location?.start?.line) && isEqual(location.start, location.end);
const isAnonymousName = (name) => /^\(anonymous_\d+\)$/.test(name);

const locationText = (lines, { start, end }) => {
  if (start.line === end.line) return lines[start.line - 1].slice(start.column, end.column);
  return [
    lines[start.line - 1].slice(start.column),
    ...lines.slice(start.line, end.line - 1),
    lines[end.line - 1].slice(0, end.column),
  ].join('\n');
};

// Top-level keys that differ by design. All other keys must be equal.
const METADATA_KEYS = new Set(['hash', '_coverageSchema']);

const compareStrict = (reference, actual) =>
  [...new Set([...Object.keys(reference), ...Object.keys(actual)])]
    .filter((key) => !METADATA_KEYS.has(key))
    .filter((key) => !isEqual(reference[key], actual[key]))
    .map((key) => `${key} differs`);

const compareFunctions = (reference, actual, seen) => {
  const errors = [];
  for (const [id, expected] of Object.entries(reference.fnMap)) {
    const found = actual.fnMap[id];
    if (!found || !isEqual(expected.loc, found.loc)) {
      errors.push(`fnMap.${id}: location differs`);
      continue;
    }
    const inferred = isAnonymousName(expected.name) && !isAnonymousName(found.name);
    if (expected.name !== found.name) {
      if (inferred) seen.add('inferred function names');
      else errors.push(`fnMap.${id}: name ${found.name}, expected ${expected.name}`);
    }
    if (!isEqual(expected.decl, found.decl)) {
      const widenedMethodSpan =
        inferred &&
        expected.decl.end.column === expected.decl.start.column + 1 &&
        comparePositions(found.decl.start, expected.decl.start) >= 0 &&
        comparePositions(found.decl.end, found.loc.start) <= 0;
      if (widenedMethodSpan) seen.add('full method spans');
      else errors.push(`fnMap.${id}: decl differs`);
    }
  }
  if (Object.keys(actual.fnMap).length !== Object.keys(reference.fnMap).length) {
    errors.push('fnMap: function count differs');
  }
  return errors;
};

const branchMatches = (expected, found, seen) => {
  if (
    expected.type !== found.type ||
    !isEqual(expected.loc, found.loc) ||
    expected.locations.length !== found.locations.length
  ) {
    return false;
  }
  return expected.locations.every((location, index) => {
    const candidate = found.locations[index];
    if (isEqual(location, candidate)) return true;
    if (isEmptyLocation(location) && isZeroWidthLocation(candidate)) {
      seen.add('real implicit else locations');
      return true;
    }
    return false;
  });
};

const isDocumentedExtraBranch = (lines, entry) => {
  const text = locationText(lines, entry.loc);
  if (entry.type === 'optional-chain') return text.includes('?.');
  return entry.type === 'binary-expr' && /\?\?=|\|\|=|&&=/.test(text);
};

const compareBranches = (reference, actual, lines, seen) => {
  const errors = [];
  const available = new Set(Object.keys(actual.branchMap));
  for (const [id, expected] of Object.entries(reference.branchMap)) {
    const match = [...available].find((candidate) =>
      branchMatches(expected, actual.branchMap[candidate], seen),
    );
    if (match === undefined) {
      errors.push(`branchMap.${id}: no matching ${expected.type} branch`);
      continue;
    }
    available.delete(match);
    if (!isEqual(reference.b[id], actual.b[match])) {
      errors.push(`b.${id}: counts ${actual.b[match]}, expected ${reference.b[id]}`);
    }
  }
  for (const id of available) {
    const entry = actual.branchMap[id];
    if (isDocumentedExtraBranch(lines, entry)) {
      seen.add(entry.type === 'optional-chain' ? 'optional-chain branches' : 'logical-assignment branches');
    } else {
      errors.push(`branchMap.${id}: extra ${entry.type} branch at line ${entry.line}`);
    }
  }
  return { errors, extra: available.size };
};

const compareDefault = (reference, actual, lines, seen) => {
  const errors = ['statementMap', 's', 'f']
    .filter((key) => !isEqual(reference[key], actual[key]))
    .map((key) => `${key} differs`);
  errors.push(...compareFunctions(reference, actual, seen));
  const branches = compareBranches(reference, actual, lines, seen);
  errors.push(...branches.errors);
  return { errors, extra: branches.extra };
};

const covered = (values) => values.filter((hits) => hits > 0).length;
const ratio = (record) => {
  const values = Object.values(record).flat();
  return `${covered(values)}/${values.length}`;
};

const istanbul = load('istanbul');
const strict = load('oxc-strict');
const oxc = load('oxc');

const errors = [];
const seen = new Set();
const rows = [];

for (const [path, reference] of Object.entries(istanbul)) {
  const file = relative(process.cwd(), path);
  if (!strict[path] || !oxc[path]) {
    errors.push(`${file}: missing from an oxc-coverage-instrument run`);
    continue;
  }
  const lines = readFileSync(path, 'utf8').split('\n');
  errors.push(...compareStrict(reference, strict[path]).map((e) => `${file} (oxc-strict): ${e}`));
  const result = compareDefault(reference, oxc[path], lines, seen);
  errors.push(...result.errors.map((e) => `${file} (oxc): ${e}`));
  rows.push({
    file,
    statements: ratio(reference.s),
    functions: ratio(reference.f),
    'branches istanbul': ratio(reference.b),
    'branches oxc-strict': ratio(strict[path].b),
    'branches oxc': ratio(oxc[path].b),
    'extra branch entries oxc': result.extra,
  });
}
for (const name of ['oxc-strict', 'oxc']) {
  const run = name === 'oxc' ? oxc : strict;
  for (const path of Object.keys(run)) {
    if (!istanbul[path]) errors.push(`${relative(process.cwd(), path)}: only in the ${name} run`);
  }
}

console.table(rows);
console.log(`Default-profile extensions seen: ${[...seen].sort().join(', ') || 'none'}`);

if (errors.length > 0) {
  console.error(errors.slice(0, 40).join('\n'));
  if (errors.length > 40) console.error(`... and ${errors.length - 40} more`);
  process.exit(1);
}
console.log(
  'PASS: oxc-strict has the same maps and hit counts as istanbul (hash and _coverageSchema differ by design). ' +
    'The default profile differs only by documented extensions.',
);
