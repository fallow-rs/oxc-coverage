#!/usr/bin/env node
// Differential check of V8 branch arm counts. For each source in the corpus,
// this script captures real `node:inspector` precise coverage, converts it with
// `v8ToIstanbul`, and runs the same source instrumented by
// istanbul-lib-instrument. Every branch arm count must be equal, keyed by the
// branch type and location. The only accepted differences are the documented
// ones in README part 6, listed in `allowedDifference` below.

import assert from 'node:assert/strict';
import inspector from 'node:inspector';
import { promisify } from 'node:util';
import vm from 'node:vm';

import { createInstrumenter } from 'istanbul-lib-instrument';

import { v8ToIstanbul } from '../crates/oxc_coverage_instrument_napi/index.js';

const cases = [
  {
    name: 'ternary with line breaks before ? and :',
    code: `function pick(v) {
  return v > 1
    ? 'big'
    : 'small';
}
for (const v of [0, 1, 2, 3, 4]) pick(v);`,
  },
  {
    name: 'ternary with line breaks after ? and :',
    code: `function pick(v) {
  return v > 1 ?
    'big' :
    'small';
}
for (const v of [0, 2, 3]) pick(v);`,
  },
  {
    name: 'ternary with comments next to ? and :',
    code: `function pick(v) {
  return v > 1 /* test */ ? /* yes */ 'big' /* no */ : // other
    'small';
}
for (const v of [0, 1, 2]) pick(v);`,
  },
  {
    name: 'ternary with short arms',
    code: `function a(v) { return v ? v : 0; }
function b(v) { return v ? 1 : 0; }
function c(v) { return !v ? 0 : v; }
for (const v of [0, 0, 5, 6, 7]) { a(v); b(v); c(v); }`,
  },
  {
    name: 'nested ternaries',
    code: `function grade(n) {
  return n > 90 ? 'a' : n > 80 ? 'b' : n > 70 ? 'c' : 'd';
}
function inner(n) {
  return n > 0 ? (n > 5 ? 'many' : 'few') : 'none';
}
for (const n of [95, 85, 85, 75, 75, 75, 10]) grade(n);
for (const n of [0, 1, 2, 9]) inner(n);`,
  },
  {
    name: 'ternary arms that contain logical expressions',
    code: `function f(a, b, c) {
  return a ? b && c : b || c;
}
function g(a, b) {
  return a ? b ?? 'fallback' : (b && b.x) || 'none';
}
f(1, 1, 1); f(1, 0, 1); f(0, 0, 1); f(0, 1, 0); f(0, 0, 0);
g(1, null); g(1, 2); g(0, { x: 1 }); g(0, null);`,
  },
  {
    name: 'logical chains',
    code: `function and(a, b, c) { return a && b && c; }
function or(a, b, c) { return a || b || c; }
function nullish(a, b, c) { return a ?? b ?? c; }
function mixed(a, b, c) { return (a && b) || c; }
for (const [a, b, c] of [[1, 1, 1], [1, 0, 1], [0, 1, 1], [0, 0, 0], [null, 2, 3], [null, null, 3]]) {
  and(a, b, c); or(a, b, c); nullish(a, b, c); mixed(a, b, c);
}`,
  },
  {
    name: 'else-if chains',
    code: `function kind(n) {
  if (n < 0) {
    return 'negative';
  } else if (n === 0) {
    return 'zero';
  } else if (n < 10) {
    return 'small';
  } else {
    return 'large';
  }
}
function noElse(n) {
  let out = 'x';
  if (n === 1) {
    out = 'one';
  } else if (n === 2) {
    out = 'two';
  }
  return out;
}
for (const n of [-1, 0, 0, 3, 3, 3, 20]) kind(n);
for (const n of [1, 2, 2, 3, 3, 3]) noElse(n);`,
  },
  {
    name: 'if without braces',
    code: `function f(n) {
  let out = 0;
  if (n > 1) out = 1;
  if (n > 2) out = 2; else out = 3;
  if (n > 3)
    out = 4;
  else if (n > 4)
    out = 5;
  return out;
}
for (const n of [0, 2, 3, 4, 5, 6]) f(n);`,
  },
  {
    name: 'switch with fallthrough and empty cases',
    code: `function sw(n) {
  let out = 0;
  switch (n) {
    case 1:
    case 2:
      out += 1;
    case 3:
      out += 2;
      break;
    case 4:
      break;
    case 5:
    default:
      out = -1;
  }
  return out;
}
function noDefault(n) {
  switch (n) {
    case 'a':
      return 1;
    case 'b':
      return 2;
  }
  return 0;
}
for (const n of [1, 2, 2, 3, 4, 5, 9, 9]) sw(n);
for (const n of ['a', 'b', 'b', 'c']) noDefault(n);`,
  },
  {
    name: 'early return before a branch',
    code: `function f(n) {
  if (n === 0) return 'zero';
  if (n > 5) {
    return 'big';
  }
  return n > 2 ? 'mid' : 'low';
}
for (const n of [0, 0, 1, 3, 4, 6, 7, 8]) f(n);`,
  },
  {
    name: 'break and continue before a branch',
    code: `function loop(values) {
  let total = 0;
  for (const v of values) {
    if (v < 0) continue;
    if (v > 100) break;
    total += v > 10 ? v : 1;
    if (v === 5) {
      total += 5;
    } else {
      total -= 1;
    }
  }
  return total;
}
loop([1, -1, 5, 20, -2, 3, 200, 4]);
loop([5, 5, -1]);`,
  },
  {
    name: 'arrow expression bodies',
    code: `const sign = (n) => n > 0 ? 1 : n < 0 ? -1 : 0;
const first = (a, b) => a || b;
const both = (a, b) => a && b ? 'both' : 'not';
for (const n of [-2, -1, 0, 1, 2, 3]) { sign(n); first(n, 9); both(n, n > 1); }`,
  },
  {
    name: 'instance class fields',
    code: `let flag = 0;
class Box {
  kind = flag > 1 ? 'many' : 'few';
  label = flag && 'set';
  size(n) {
    return n > 1 ? n : 1;
  }
}
for (const n of [0, 1, 2, 3]) {
  flag = n;
  new Box().size(n);
}`,
  },
  {
    name: 'static class fields',
    // V8 reports the instance and the static initializer with the same range
    // (the class body), so an arm in a static field takes its count from the
    // instance initializer. The fix needs the static member spans from the
    // parser.
    knownDifference: 'a static field arm gets the count of the instance initializer',
    code: `let flag = 0;
class Box {
  kind = 1;
  static base = flag ? 1 : 2;
}
for (const n of [0, 1, 2, 3]) {
  flag = n;
  new Box();
}`,
  },
  {
    name: 'top-level code',
    code: `let total = 0;
const items = [1, 2, 3, 4, 5];
for (const item of items) {
  if (item % 2) {
    total += item;
  } else {
    total -= item;
  }
  total += item > 3 ? 1 : 0;
}
const result = total > 0 && 'positive';
globalThis.out = result || 'other';`,
  },
  {
    name: 'try and catch',
    code: `function risky(n) {
  try {
    if (n > 2) throw new Error('big');
    return n ? 'some' : 'none';
  } catch (error) {
    return error.message === 'big' ? 'caught' : 'other';
  } finally {
    globalThis.last = n > 1 || 'small';
  }
}
for (const n of [0, 1, 2, 3, 4]) risky(n);`,
  },
  {
    name: 'UTF-16 source text',
    code: `const prefix = '😀😀😀 é ✓';
function label(n) {
  const mark = n > 1 ? '✓✓' : '✗';
  if (n === 0) {
    return '😀' + mark;
  }
  return prefix.length > 3 && n > 2 ? 'é' : mark;
}
for (const n of [0, 1, 2, 3, 4]) label(n);`,
  },
  {
    name: 'istanbul ignore on one arm',
    code: `function pick(v) {
  return v ? /* istanbul ignore next */ 'yes' : 'no';
}
function pickElse(v) {
  return v ? 'yes' : /* istanbul ignore next */ 'no';
}
function guard(v) {
  /* istanbul ignore else */
  if (v) {
    return 1;
  } else {
    return 2;
  }
}
function guardIf(v) {
  /* istanbul ignore if */
  if (v) {
    return 1;
  }
  return 2;
}
for (const v of [0, 1, 1]) { pick(v); pickElse(v); guard(v); guardIf(v); }`,
  },
  {
    name: 'default values',
    code: `function greet(name = 'world', punct = name ? '!' : '?') {
  return name + punct;
}
const arrow = (n = 1) => n;
greet(); greet('a'); greet('b', '.'); arrow(); arrow(2);`,
  },
  {
    name: 'optional chains and logical assignment',
    code: `function read(o) {
  let v = o?.a?.b;
  v ||= 'empty';
  v &&= v + '!';
  let w = o?.c;
  w ??= 0;
  return o ? v : w;
}
read(null); read({}); read({ a: { b: 1 } }); read({ a: {} });`,
  },
];

const sourceUrl = (index) => `oxc-coverage-instrument://v8-branch-parity-${index}.js`;
const fileName = (index) => `v8-branch-parity-${index}.js`;

const captureV8 = async (code, url) => {
  const session = new inspector.Session();
  const post = promisify(session.post).bind(session);
  session.connect();
  try {
    await post('Profiler.enable');
    await post('Profiler.startPreciseCoverage', { callCount: true, detailed: true });
    vm.runInContext(`${code}\n//# sourceURL=${url}\n`, vm.createContext({}));
    const { result } = await post('Profiler.takePreciseCoverage');
    const scripts = result.filter((entry) => entry.url === url);
    assert.equal(scripts.length, 1, `precise coverage must contain one script for ${url}`);
    return scripts[0].functions;
  } finally {
    await post('Profiler.stopPreciseCoverage').catch(() => undefined);
    await post('Profiler.disable').catch(() => undefined);
    session.disconnect();
  }
};

const runIstanbul = (code, name) => {
  const instrumenter = createInstrumenter({ esModules: false, produceSourceMap: false });
  const instrumented = instrumenter.instrumentSync(code, name);
  const context = vm.createContext({});
  vm.runInContext(instrumented, context);
  const coverage = context.__coverage__?.[name];
  assert(coverage, `istanbul-lib-instrument must record coverage for ${name}`);
  return coverage;
};

const position = ({ line, column }) => `${line}:${column}`;
const range = (loc) => `${position(loc.start)}-${position(loc.end)}`;

// Istanbul writes a synthetic `else` as `{ start: {}, end: {} }`. At runtime
// the position objects can carry keys with `undefined` values.
const isEmptyLocation = (loc) =>
  loc.start.line === undefined &&
  loc.start.column === undefined &&
  loc.end.line === undefined &&
  loc.end.column === undefined;
const isZeroWidth = (loc) =>
  Number.isInteger(loc.start.line) &&
  loc.start.line === loc.end.line &&
  loc.start.column === loc.end.column;

const branchesByKey = (coverage) => {
  const branches = new Map();
  for (const [id, branch] of Object.entries(coverage.branchMap)) {
    const key = `${branch.type} ${range(branch.loc)}`;
    assert(!branches.has(key), `duplicate branch key ${key}`);
    branches.set(key, { branch, counts: coverage.b[id] });
  }
  return branches;
};

const isBefore = (a, b) => a.line < b.line || (a.line === b.line && a.column <= b.column);

// The Istanbul call count of the function that owns a parameter default. An
// Istanbul function location is the body, so the owner is the function whose
// body starts first after the default value.
const parameterOwnerCount = (coverage, loc) => {
  let best = null;
  for (const [id, fn] of Object.entries(coverage.fnMap)) {
    if (!isBefore(loc.end, fn.loc.start)) continue;
    if (best === null || isBefore(fn.loc.start, coverage.fnMap[best].loc.start)) best = id;
  }
  return best === null ? null : coverage.f[best];
};

const sourceSlice = (code, loc) => {
  const lines = code.split('\n');
  if (loc.start.line === loc.end.line) {
    return lines[loc.start.line - 1].slice(loc.start.column, loc.end.column);
  }
  return [
    lines[loc.start.line - 1].slice(loc.start.column),
    ...lines.slice(loc.start.line, loc.end.line - 1),
    lines[loc.end.line - 1].slice(0, loc.end.column),
  ].join('\n');
};

const LOGICAL_ASSIGNMENT = /(\|\||&&|\?\?)=/;

// README part 6 documents these differences. Any other difference fails.
// Each rule must match at least once, so that the corpus keeps them tested.
const allowedDifference = {
  // Istanbul gives a synthetic `else` an empty location. The default profile
  // gives it a zero-width location, as in the istanbul-diff default gate. The
  // count is still compared.
  syntheticElseLocation: (expected, actual) => isEmptyLocation(expected) && isZeroWidth(actual),
  // V8 has no range for a default value, so the arm gets the call count of
  // its function. Istanbul counts only the calls that use the default.
  defaultArgCount: (type, v8Count, functionCount) =>
    type === 'default-arg' && v8Count === functionCount,
  // When a pragma removes the `if` arm, the count of that arm is unknown, so
  // the implicit `else` does not take the remainder and gets 0.
  ignoredIfElseCount: (branch, v8Count) =>
    branch.type === 'if' &&
    branch.locations.length === 1 &&
    isEmptyLocation(branch.locations[0]) &&
    v8Count === 0,
  // Optional chains and logical assignments are extensions of the default
  // profile. Istanbul has no branch for them.
  extensionBranch: (type, code, loc) =>
    type === 'optional-chain' ||
    (type === 'binary-expr' && LOGICAL_ASSIGNMENT.test(sourceSlice(code, loc))),
};

const compareCase = (testCase, v8Coverage, istanbulCoverage) => {
  const failures = [];
  const v8Branches = branchesByKey(v8Coverage);
  const istanbulBranches = branchesByKey(istanbulCoverage);
  let compared = 0;

  for (const [key, expected] of istanbulBranches) {
    const actual = v8Branches.get(key);
    if (!actual) {
      failures.push(`${key}: missing from v8ToIstanbul output`);
      continue;
    }
    const expectedArms = expected.branch.locations;
    const actualArms = actual.branch.locations;
    const sameArms =
      expectedArms.length === actualArms.length &&
      expectedArms.every((loc, arm) => {
        if (range(loc) === range(actualArms[arm])) return true;
        if (!allowedDifference.syntheticElseLocation(loc, actualArms[arm])) return false;
        used.add('syntheticElseLocation');
        return true;
      });
    if (!sameArms) {
      failures.push(
        `${key}: arm locations [${actualArms.map(range)}] differ from Istanbul [${expectedArms.map(range)}]`,
      );
      continue;
    }
    expected.counts.forEach((expectedCount, arm) => {
      compared += 1;
      const actualCount = actual.counts[arm];
      if (actualCount === expectedCount) return;
      const loc = expected.branch.locations[arm];
      const functionCount = parameterOwnerCount(istanbulCoverage, loc);
      if (allowedDifference.defaultArgCount(expected.branch.type, actualCount, functionCount)) {
        used.add('defaultArgCount');
        return;
      }
      if (allowedDifference.ignoredIfElseCount(expected.branch, actualCount)) {
        used.add('ignoredIfElseCount');
        return;
      }
      failures.push(
        `${key} arm ${arm} at ${range(loc)}: v8ToIstanbul ${actualCount}, Istanbul ${expectedCount}`,
      );
    });
  }

  for (const [key, actual] of v8Branches) {
    if (istanbulBranches.has(key)) continue;
    if (allowedDifference.extensionBranch(actual.branch.type, testCase.code, actual.branch.loc)) {
      used.add('extensionBranch');
      continue;
    }
    failures.push(`${key}: not reported by Istanbul and not a documented extension`);
  }

  return { failures, compared };
};

const used = new Set();
let failed = 0;
let arms = 0;
for (const [index, testCase] of cases.entries()) {
  const url = sourceUrl(index);
  const name = fileName(index);
  const functions = await captureV8(testCase.code, url);
  const v8Coverage = JSON.parse(v8ToIstanbul(testCase.code, name, JSON.stringify(functions)));
  const istanbulCoverage = runIstanbul(testCase.code, name);
  const { failures, compared } = compareCase(testCase, v8Coverage, istanbulCoverage);
  assert(compared > 0, `case "${testCase.name}" must compare at least one branch arm`);
  arms += compared;
  if (testCase.knownDifference) {
    if (failures.length > 0) {
      console.log(`gap  ${testCase.name}: ${testCase.knownDifference}`);
      continue;
    }
    failed += 1;
    console.log(`FAIL ${testCase.name}: now matches Istanbul, remove knownDifference`);
    continue;
  }
  if (failures.length === 0) {
    console.log(`ok   ${testCase.name}`);
    continue;
  }
  failed += 1;
  console.log(`FAIL ${testCase.name}`);
  for (const failure of failures) console.log(`     ${failure}`);
}

for (const rule of Object.keys(allowedDifference)) {
  if (used.has(rule)) continue;
  failed += 1;
  console.log(`FAIL allowed difference "${rule}" matched no case; update the corpus or the list`);
}

if (failed > 0) {
  console.error(`FAIL: ${failed} V8 branch parity checks differ from Istanbul.`);
  process.exit(1);
}
const gaps = cases.filter((testCase) => testCase.knownDifference).length;
console.log(
  `PASS: V8 branch arm counts match Istanbul in ${cases.length - gaps} cases (${arms} arms), ${gaps} known gap.`,
);
