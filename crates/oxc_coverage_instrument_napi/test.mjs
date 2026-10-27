// Quick test for the napi bindings
import {
  instrument,
  remapCoverageMap,
  remapCoverageMapWithLoader,
  v8ToIstanbul,
  v8ToIstanbulWithLoader,
  createReport,
  summarizeCoverage,
  writeHtmlReport,
} from './index.js';
import { createOxcInstrumenter } from './vitest.js';
import { Instrumenter as KarmaInstrumenter, createKarmaInstrumenter } from './karma.js';
import { createRequire } from 'node:module';
import { strict as assert } from 'node:assert';
import libCoverage from 'istanbul-lib-coverage';
import { createInstrumenter } from 'istanbul-lib-instrument';
import { existsSync, statSync, readFileSync, writeFileSync, rmSync, mkdirSync, mkdtempSync } from 'node:fs';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { dirname, join } from 'node:path';
import { tmpdir } from 'node:os';

const __dirname = dirname(fileURLToPath(import.meta.url));
const { createCoverageMap } = libCoverage;
console.log('Testing oxc-coverage-instrument napi bindings...\n');

// Log which local napi artifacts exist before the binding loader runs. This
// is a permanent CI safeguard, not debug output: when the wasm32-wasi job
// has no local artifact the loader silently falls back to the published
// `@oxc-coverage-instrument/binding-wasm32-wasi` optionalDependency and
// silently exercises the previous release. Surfacing the file list up front
// catches that regression class on the first failing run instead of after
// hours of remote diagnosis. Companion safeguard: the wasi CI job deletes
// the published wasm between build and test (see `.github/workflows/ci.yml`).
for (const f of [
  'coverage-instrument.wasm32-wasi.wasm',
  'coverage-instrument.wasm32-wasi.debug.wasm',
  'coverage-instrument.linux-x64-gnu.node',
  'coverage-instrument.darwin-arm64.node',
]) {
  const p = join(__dirname, f);
  if (existsSync(p)) console.log(`[napi-artifact] ${f} (${statSync(p).size} bytes)`);
}
console.log();

function runInstrumented(result, filename, callExpression) {
  const sharedGlobal = {};
  new Function('globalThis', `${result.code}\n${callExpression}`)(sharedGlobal);
  return sharedGlobal.__coverage__[filename];
}

// Test: Basic instrumentation
{
  const result = instrument('function add(a, b) { return a + b; }', 'test.js');
  assert(result.code.includes('cov_'), 'Code should contain coverage counter');
  const coverageMap = JSON.parse(result.coverageMap);
  assert.equal(coverageMap.path, 'test.js');
  assert.equal(Object.keys(coverageMap.fnMap).length, 1);
  assert.equal(coverageMap.fnMap['0'].name, 'add');
  console.log('  [PASS] Basic instrumentation');
}

{
  const terminators = [
    ['LF', '\n'],
    ['CRLF', '\r\n'],
    ['CR', '\r'],
    ['LS', '\u2028'],
    ['PS', '\u2029'],
  ];
  const locations = (coverage) => Object.values(coverage.statementMap).map(({ start, end }) => ({
    start,
    end,
  }));

  for (const [name, terminator] of terminators) {
    const source = `first();${terminator}second();`;
    const actual = JSON.parse(instrument(source, `${name}.js`).coverageMap);
    const referenceInstrumenter = createInstrumenter();
    referenceInstrumenter.instrumentSync(source, `${name}.js`);
    const expected = referenceInstrumenter.lastFileCoverage();
    assert.deepEqual(locations(actual), locations(expected), `${name} locations must match Istanbul`);
  }
  console.log('  [PASS] ECMAScript line terminators match Istanbul locations');
}

// Test: With options
{
  const result = instrument('const x = 1;', 'test.js', {
    coverageVariable: '__custom_cov__',
  });
  assert(result.code.includes('__custom_cov__'), 'Should use custom coverage variable');
  console.log('  [PASS] Custom coverage variable');
}

// Test: Source map
{
  const result = instrument('function f() { return 1; }', 'test.js', {
    sourceMap: true,
  });
  assert(result.sourceMap, 'Should have source map');
  const sm = JSON.parse(result.sourceMap);
  assert.equal(sm.version, 3);
  console.log('  [PASS] Source map generation');
}

// Test: TypeScript
{
  const result = instrument(
    'function add(a: number, b: number): number { return a + b; }',
    'test.ts',
  );
  const coverageMap = JSON.parse(result.coverageMap);
  assert.equal(coverageMap.fnMap['0'].name, 'add');
  console.log('  [PASS] TypeScript support');
}

// Test: Error handling
{
  try {
    instrument('function {{{', 'bad.js');
    assert.fail('Should have thrown');
  } catch (e) {
    assert(e.message.includes('parse error'), `Expected parse error, got: ${e.message}`);
    console.log('  [PASS] Parse error handling');
  }
}

// Test: Istanbul format compliance
{
  const result = instrument('function f() { if (true) { return 1; } else { return 0; } }', 'test.js');
  const cm = JSON.parse(result.coverageMap);
  assert(cm.statementMap, 'Must have statementMap');
  assert(cm.fnMap, 'Must have fnMap');
  assert(cm.branchMap, 'Must have branchMap');
  assert(cm.s, 'Must have s');
  assert(cm.f, 'Must have f');
  assert(cm.b, 'Must have b');
  assert.equal(cm.branchMap['0'].type, 'if');
  assert.equal(cm.branchMap['0'].locations.length, 2);
  console.log('  [PASS] Istanbul format compliance');
}

// Test: Default-arg branches increment at runtime
{
  const result = instrument('function f(x = 1) { return x; }\nconst obj = {};\nconst { y = 2 } = obj;\nf();', 'default-arg.js');
  const context = { globalThis: {} };
  const runner = new Function('globalThis', `${result.code}\nreturn globalThis.__coverage__;`);
  const coverage = runner(context.globalThis);
  assert.equal(coverage['default-arg.js'].b['0'][0], 1, 'Default parameter should hit branch counter');
  assert.equal(coverage['default-arg.js'].b['1'][0], 1, 'Destructuring default should hit branch counter');
  console.log('  [PASS] Default-arg runtime branch counters');
}

// Test: Parenthesized initializers keep their inferred function names
{
  const result = instrument(
    'const f = (function () {});\nconst g = (() => 1);\nglobalThis.names = [f.name, g.name];',
    'paren-names.js',
    { sourceType: 'script' },
  );
  const context = {};
  new Function('globalThis', result.code)(context);
  assert.deepEqual(context.names, ['f', 'g']);
  console.log('  [PASS] Parenthesized initializers keep inferred names');
}

// Test: Same path with changed shape refreshes stale coverage data
{
  const first = instrument('function f() { return 1; }\nf();', 'same.js');
  const second = instrument('function f() { if (true) { return 1; } return 0; }\nf();', 'same.js');
  const sharedGlobal = {};
  new Function('globalThis', first.code)(sharedGlobal);
  new Function('globalThis', second.code)(sharedGlobal);
  assert.ok(sharedGlobal.__coverage__['same.js'].b['0'], 'Updated instrumentation should refresh branch data for the same path');
  console.log('  [PASS] Stale coverage refresh by hash');
}

// Test: magic coverage storage keys cannot use inherited prototype state
{
  for (const coverageVariable of [
    '__proto__',
    'constructor',
    'hasOwnProperty',
    'isPrototypeOf',
    'propertyIsEnumerable',
    'toLocaleString',
    'toString',
    'valueOf',
  ]) {
    assert.throws(
      () => instrument('const x = 1;', 'magic-variable.js', { coverageVariable }),
      /invalid coverage variable.*Object\.prototype/,
      `${coverageVariable} must be rejected`,
    );
  }

  const first = instrument('globalThis.first = 1;', '__proto__');
  const second = instrument('globalThis.second = 2; globalThis.third = 3;', '__proto__');
  const sharedGlobal = {};
  new Function('globalThis', first.code)(sharedGlobal);
  const firstCoverage = sharedGlobal.__coverage__.__proto__;
  new Function('globalThis', second.code)(sharedGlobal);
  const replacedCoverage = sharedGlobal.__coverage__.__proto__;
  new Function('globalThis', second.code)(sharedGlobal);
  const store = sharedGlobal.__coverage__;
  assert.equal(Object.hasOwn(store, '__proto__'), true, 'filename must create an own slot');
  assert.deepEqual(Object.keys(store), ['__proto__'], 'filename slot must be enumerable');
  assert.notEqual(store.__proto__, firstCoverage, 'changed coverage shape must replace stale data');
  assert.equal(store.__proto__, replacedCoverage, 'matching coverage hash must reuse current data');
  assert.equal(store.__proto__.path, '__proto__');
  console.log('  [PASS] Magic coverage storage keys use own properties');
}

// Test: Issue regressions that require runtime counter execution
{
  const noElse = instrument(
    `function f(x) {
      if (!x.roles) return x
      return x.roles.map(r => r)
    }`,
    'issue-19.js',
  );
  const noElseCoverage = runInstrumented(
    noElse,
    'issue-19.js',
    "eval('f')({ roles: ['a'] }); eval('f')({});",
  );
  assert.deepEqual(noElseCoverage.b['0'], [1, 1], 'if without else should hit both branch counters');
  const noElseSynthetic = JSON.parse(noElse.coverageMap).branchMap['0'].locations[1];
  assert.ok(
    Number.isInteger(noElseSynthetic.start.line),
    'synthetic else arm must carry a real start line, not an empty placeholder',
  );
  assert.deepEqual(
    noElseSynthetic.start,
    noElseSynthetic.end,
    'synthetic else arm is anchored as a zero-width location',
  );

  const objectMethod = instrument(
    `const obj = {
      /* v8 ignore next -- @preserve */
      method(x) {
        const y = x.foo
        if (y) {
          y.bar = 1
        }
      },
    }`,
    'issue-20.js',
  );
  const objectMethodMap = JSON.parse(objectMethod.coverageMap);
  assert.equal(Object.keys(objectMethodMap.fnMap).length, 0, 'ignored object method should not add fnMap entries');
  assert.equal(Object.keys(objectMethodMap.branchMap).length, 0, 'ignored object method should not add branchMap entries');

  const ternary = instrument(
    `function f(x) {
      return {
        ...x,
        ...(x.set
          ? { a: 1 }
          : /* v8 ignore next -- @preserve */
            {}),
      }
    }`,
    'issue-21.js',
  );
  const ternaryMap = JSON.parse(ternary.coverageMap);
  assert.equal(Object.keys(ternaryMap.fnMap).length, 1, 'enclosing function should still be tracked');
  // Istanbul keeps the branch entry when only one arm is ignored: the
  // surviving arm still gets a location and a counter.
  assert.equal(Object.keys(ternaryMap.branchMap).length, 1, 'surviving arm should keep its branch entry');
  assert.equal(ternaryMap.branchMap['0'].locations.length, 1, 'ignored arm must be dropped from locations');
  assert.equal(ternaryMap.b['0'].length, 1, 'branch hit array must match locations length');

  const fullyIgnoredTernary = instrument(
    'function f(x) { return x ? /* v8 ignore next */ 1 : /* v8 ignore next */ 2; }',
    'fully-ignored-ternary.js',
  );
  assert.equal(Object.keys(JSON.parse(fullyIgnoredTernary.coverageMap).branchMap).length, 0, 'empty branches should be pruned');

  const logicalLeaf = instrument(
    'function f(a, b) { return a && /* v8 ignore next */ b; }',
    'logical-leaf.js',
  );
  const logicalMap = JSON.parse(logicalLeaf.coverageMap);
  assert.equal(
    Object.keys(logicalMap.branchMap).length,
    1,
    'logical branch entry must survive when one operand is ignored',
  );
  assert.equal(logicalMap.branchMap['0'].locations.length, 1, 'ignored leaf must be dropped from locations');
  assert.equal(logicalMap.b['0'].length, 1, 'branch hit array must match locations length');

  // Pragma scoped to a single arm: the branch entry survives with one
  // remaining arm. Pragma attached to a JSX attribute or child wraps the
  // whole subtree and the branch is dropped entirely.
  for (const [label, source, path, expectedBranches, expectedArms] of [
    ['nullish rhs', 'function f(x) { return x ?? /* v8 ignore next -- @preserve */ [] }', 'issue-27-nullish.js', 1, 1],
    ['or rhs', 'function f(x) { return x || /* v8 ignore next -- @preserve */ true }', 'issue-27-or.js', 1, 1],
    ['and rhs', 'function f(x) { return x && /* v8 ignore next -- @preserve */ true }', 'issue-27-and.js', 1, 1],
    ['conditional rhs', 'function f(x) { return x ? 1 : /* v8 ignore next -- @preserve */ 2 }', 'issue-27-cond.js', 1, 1],
    [
      'jsx attribute',
      `function f(pass) {
        return <Tag
          /* v8 ignore next -- @preserve */
          text={pass ? 'Pass' : 'Fail'}
        />
      }`,
      'issue-28.tsx',
      0,
      0,
    ],
    [
      'jsx child',
      `function f(x) {
        return <div>
          {/* v8 ignore next -- @preserve */}
          {x ? <a/> : <b/>}
        </div>
      }`,
      'issue-29.tsx',
      0,
      0,
    ],
  ]) {
    const map = JSON.parse(instrument(source, path).coverageMap);
    assert.equal(
      Object.keys(map.branchMap).length,
      expectedBranches,
      `${label}: unexpected branch entry count`,
    );
    if (expectedBranches > 0) {
      assert.equal(
        map.branchMap['0'].locations.length,
        expectedArms,
        `${label}: surviving arm count mismatch`,
      );
    }
  }

  const cachedSetup = instrument('function f() { return 1; }\nf();', 'issue-34.js');
  const covName = cachedSetup.code.match(/var (cov_[a-f0-9]+)/)?.[1];
  assert(covName, 'instrumented code should declare a coverage binding');
  assert(cachedSetup.code.includes('return actualCoverage; })();'), 'coverage setup should be invoked once');
  assert(!cachedSetup.code.includes(`${covName}().`), 'counter sites should use cached coverage data');
  const issue34Coverage = runInstrumented(cachedSetup, 'issue-34.js', "eval('f')();");
  assert.equal(issue34Coverage.f['0'], 2, 'cached coverage object should still record runtime function hits');
  assert.equal(issue34Coverage.s['0'], 2, 'cached coverage object should still record runtime statement hits');

  const classMethod = instrument(
    `class C {
      /* istanbul ignore next */
      render(x) {
        if (x) { return 1; }
        return 2;
      }

      update() { return 3; }
    }`,
    'class-method-ignore.js',
  );
  const classMethodMap = JSON.parse(classMethod.coverageMap);
  assert.equal(
    Object.keys(classMethodMap.fnMap).length,
    1,
    'ignored class method should not add a fnMap entry',
  );
  assert.equal(
    Object.keys(classMethodMap.branchMap).length,
    0,
    'ignored class method body should not add branches',
  );
  assert.equal(
    Object.keys(classMethodMap.statementMap).length,
    1,
    'ignored class method body should not add statements',
  );

  const ignoreClassMethods = instrument(
    `function TestClass() {}
    TestClass.prototype.testMethod = function testMethod(i) { return i; };
    TestClass.prototype.goodMethod = function goodMethod(i) { return i; };
    var testClass = new TestClass();
    testClass.goodMethod();
    testClass.testMethod(1);`,
    'ignore-class-methods.js',
    { ignoreClassMethods: ['testMethod'] },
  );
  const ignoreClassMethodsMap = JSON.parse(ignoreClassMethods.coverageMap);
  assert.deepEqual(
    Object.values(ignoreClassMethodsMap.fnMap).map((entry) => entry.name),
    ['TestClass', 'goodMethod'],
    'ignoreClassMethods should skip matching named function expressions',
  );
  assert.deepEqual(
    Object.values(ignoreClassMethodsMap.statementMap).map((loc) => loc.start.line),
    [2, 3, 3, 4, 5, 6],
    'ignoreClassMethods should skip matching function expression bodies',
  );

  const ignoreIf = instrument(
    `function f(x) {
      /* istanbul ignore if */
      if (x) return 1;
      return 2;
    }`,
    'ignore-if.js',
  );
  const ignoreIfMap = JSON.parse(ignoreIf.coverageMap);
  assert.equal(ignoreIfMap.branchMap['0'].locations.length, 1, 'ignore if should keep only the alternate path');
  assert.equal(
    Object.keys(ignoreIfMap.statementMap).length,
    2,
    'ignore if should skip statement counters in the ignored arm',
  );

  const ignoredSwitchCase = instrument(
    `function f(item) {
      switch (item.type) {
        case 'html': return 'a'
        /* istanbul ignore next */
        default: return 'b'
      }
    }`,
    'ignored-switch-case.js',
  );
  const ignoredSwitchCaseMap = JSON.parse(ignoredSwitchCase.coverageMap);
  assert.equal(
    ignoredSwitchCaseMap.branchMap['0'].locations.length,
    1,
    'ignored switch case should be pruned from branch paths',
  );
  assert.deepEqual(
    Object.values(ignoredSwitchCaseMap.statementMap).map((loc) => loc.start.line),
    [2, 3],
    'ignored switch case statements should be pruned',
  );

  console.log('  [PASS] Issue regression runtime parity');
}

// Test: No-block loop and statement-child bodies increment their own statement counters
{
  const cases = [
    {
      name: 'while',
      filename: 'loop-while.js',
      source: 'function f() { let i = 0; while (i < 3) i++; return i; }',
      call: "eval('f')();",
    },
    {
      name: 'for',
      filename: 'loop-for.js',
      source: 'function f() { let total = 0; for (let i = 0; i < 3; i++) total++; return total; }',
      call: "eval('f')();",
    },
    {
      name: 'for-of',
      filename: 'loop-for-of.js',
      source: 'function f(items) { let total = 0; for (const x of items) total += x; return total; }',
      call: "eval('f')([1, 2, 3]);",
    },
    {
      name: 'for-in',
      filename: 'loop-for-in.js',
      source: 'function f(obj) { let total = 0; for (const k in obj) total++; return total; }',
      call: "eval('f')({ a: 1, b: 2, c: 3 });",
    },
    {
      name: 'do-while',
      filename: 'loop-do-while.js',
      source: 'function f() { let i = 0; do i++; while (i < 3); return i; }',
      call: "eval('f')();",
    },
    {
      name: 'with',
      filename: 'with-no-block.js',
      source: 'function f(obj) { with (obj) x++; return obj.x; }',
      call: "eval('f')({ x: 0 });",
    },
    {
      name: 'label',
      filename: 'label-no-block.js',
      source: 'function f() { let n = 0; label: n++; return n; }',
      call: "eval('f')();",
    },
    {
      name: 'loop-label',
      filename: 'loop-label-no-block.js',
      source: 'function f() { let n = 0; while (n < 3) label: n++; return n; }',
      call: "eval('f')();",
    },
    {
      name: 'label-loop',
      filename: 'label-loop.js',
      source: 'function f() { let n = 0; label: while (n < 3) { n++; continue label; } return n; }',
      call: "eval('f')();",
    },
  ];

  for (const item of cases) {
    const result = instrument(item.source, item.filename);
    const map = JSON.parse(result.coverageMap);
    const emittedStatementCounters = result.code.match(/\+\+cov_[a-f0-9]+\.s\[\d+\]/g) ?? [];
    assert.equal(
      emittedStatementCounters.length,
      Object.keys(map.statementMap).length,
      `${item.name} should emit every statementMap counter`,
    );

    const coverage = runInstrumented(result, item.filename, item.call);
    assert(
      Object.entries(coverage.s).every(([, hits]) => hits > 0),
      `${item.name} should hit every statement counter when the body runs`,
    );
  }

  console.log('  [PASS] No-block loop and statement-child body counters');
}

// Test: reportLogic adds bT (truthy-value tracking) for logical operands
{
  const source = 'function f(a, b) { return a && b; }';

  const off = instrument(source, 'logic.js');
  const cmOff = JSON.parse(off.coverageMap);
  const binaryIds = Object.entries(cmOff.branchMap)
    .filter(([, entry]) => entry.type === 'binary-expr')
    .map(([id]) => id);
  assert.equal(binaryIds.length, 1, 'Should record exactly one binary-expr branch for `a && b`');
  assert.equal(cmOff.bT, undefined, 'bT must be omitted when reportLogic is off');

  const on = instrument(source, 'logic.js', { reportLogic: true });
  const coverage = runInstrumented(
    on,
    'logic.js',
    'globalThis.f = f;\nf(0, 0); f(1, 0); f(1, 1);',
  );
  const [id] = binaryIds;

  // Three calls: f(0,0), f(1,0), f(1,1)
  // b[0] (a evaluated) = 3; b[1] (b evaluated when a truthy) = 2
  assert.deepEqual(coverage.b[id], [3, 2], 'b counts should reflect short-circuit semantics');
  // bT[0] (a truthy) = 2 (calls 2,3); bT[1] (b truthy) = 1 (call 3)
  assert.deepEqual(coverage.bT[id], [2, 1], 'bT counts should reflect truthy outcomes per operand');
  console.log('  [PASS] reportLogic tracks truthy hits at runtime');
}

// Test: inputSourceMap is composed into the coverage map
{
  const inputSourceMap = {
    version: 3,
    sources: ['original.ts'],
    names: ['x'],
    mappings: 'AAAA,EAAA',
    sourcesContent: ['const x: number = 1;\n'],
    file: 'pre-transform.js',
  };

  const result = instrument('const x = 1;', 'after-transform.js', {
    inputSourceMap: JSON.stringify(inputSourceMap),
  });
  const cm = JSON.parse(result.coverageMap);
  assert(cm.inputSourceMap, 'inputSourceMap must be attached to the coverage map');
  assert.equal(cm.inputSourceMap.version, 3);
  assert.deepEqual(cm.inputSourceMap.sources, inputSourceMap.sources);
  assert.equal(cm.inputSourceMap.mappings, inputSourceMap.mappings, 'mappings should pass through verbatim');
  assert.deepEqual(cm.inputSourceMap.names, inputSourceMap.names);
  assert.deepEqual(cm.inputSourceMap.sourcesContent, inputSourceMap.sourcesContent);
  console.log('  [PASS] inputSourceMap composed into coverage map');
}

// Test: composeInputSourceMap folds the input map in eagerly (issue #100)
{
  const originalTs = 'const x: number = 1;\nconst y: number = 2;\nconst z: number = 3;\n';
  const intermediateJs = 'const x = 1;\nconst y = 2;\nconst z = 3;\n';
  const inputSourceMap = {
    version: 3,
    sources: ['src/app.ts'],
    sourcesContent: [originalTs],
    mappings: 'AAAA;AACA;AACA',
    names: [],
  };

  const result = instrument(intermediateJs, 'intermediate.js', {
    inputSourceMap: JSON.stringify(inputSourceMap),
    composeInputSourceMap: true,
  });
  const cm = JSON.parse(result.coverageMap);

  // The returned coverage map is already in original-source space.
  assert.equal(cm.path, 'src/app.ts', 'composed coverage map is keyed by the original source path');
  assert(!cm.inputSourceMap, 'composed coverage map carries no embedded inputSourceMap');
  const lines = Object.values(cm.statementMap)
    .map((loc) => loc.start.line)
    .sort();
  assert.deepEqual(lines, [1, 2, 3], 'composed statement lines point at original.ts lines');

  // The runtime coverage baked into `code` is keyed by the original path too,
  // so an E2E collector can dump window.__coverage__ verbatim.
  assert(
    result.code.includes('var path = "src/app.ts"'),
    'composed preamble keys the runtime coverage by the original source path',
  );

  // remapCoverageMap on the composed result is a no-op: the entry has no
  // inputSourceMap, so it passes through unchanged under its original key.
  const passthrough = JSON.parse(remapCoverageMap(JSON.stringify({ [cm.path]: cm })));
  assert(passthrough['src/app.ts'], 'remapCoverageMap leaves the already-composed entry in place');
  assert.deepEqual(
    Object.values(passthrough['src/app.ts'].statementMap)
      .map((loc) => loc.start.line)
      .sort(),
    [1, 2, 3],
    'remapCoverageMap is a no-op on the composed result',
  );
  console.log('  [PASS] composeInputSourceMap folds the input map in eagerly');
}

// Test: composeInputSourceMap drops unmapped positions (issue #105)
{
  // The input map only carries a mapping for generated line 1 ('AAAA'); the
  // line 2+ statements have no original position. The original source is a
  // single line, so keeping them would strand entries past end-of-file (the
  // Vue-SFC compiler-boilerplate case from the issue).
  const originalTs = 'export const a = 1;\n'; // one line
  const intermediateJs = 'const a = 1;\nconst b = 2;\nconst c = 3;\n';
  const inputSourceMap = {
    version: 3,
    sources: ['src/app.ts'],
    sourcesContent: [originalTs],
    mappings: 'AAAA',
    names: [],
  };

  const eager = JSON.parse(
    instrument(intermediateJs, 'intermediate.js', {
      inputSourceMap: JSON.stringify(inputSourceMap),
      composeInputSourceMap: true,
    }).coverageMap,
  );
  assert.equal(eager.path, 'src/app.ts');
  assert.equal(
    Object.keys(eager.statementMap).length,
    1,
    'eager compose drops the two unmapped statements',
  );
  assert(
    Object.values(eager.statementMap).every((loc) => loc.start.line <= 1),
    'no surviving statement lands past the end of the original file',
  );

  // Parity with the lazy path: remapCoverageMap with dropUnmapped on the plain
  // (non-composed) instrumented map produces the same single-statement result.
  const plain = JSON.parse(
    instrument(intermediateJs, 'intermediate.js', {
      inputSourceMap: JSON.stringify(inputSourceMap),
    }).coverageMap,
  );
  const lazyDropped = JSON.parse(
    remapCoverageMap(JSON.stringify({ [plain.path]: plain }), { dropUnmapped: true }),
  );
  assert.equal(
    Object.keys(lazyDropped['src/app.ts'].statementMap).length,
    1,
    'lazy dropUnmapped agrees with eager compose',
  );
  console.log('  [PASS] composeInputSourceMap drops unmapped positions (issue #105)');
}

// Test: composeInputSourceMap-emitted code RUNS without crashing when a
// branch on an unmapped line is dropped (issue #106). A trimmed map whose code
// still increments the dropped branch counter leaves `cov.b[id]` undefined and
// throws a TypeError at runtime.
{
  const generated = [
    'const a = 1;',
    'const b = 2;',
    'const c = 3;',
    "const w = a > 0 ? 'pos' : 'neg';", // line 4: unmapped, contains a branch
    'globalThis.__ran = (a + b + c + w.length);',
    '',
  ].join('\n');
  // Lines 1-3 map to a 3-line original; lines 4-5 are unmapped.
  const inputSourceMap = JSON.stringify({
    version: 3,
    sources: ['original.ts'],
    names: [],
    mappings: 'AAAA;AACA;AACA;;;',
    sourcesContent: ['const a = 1;\nconst b = 2;\nconst c = 3;\n'],
  });

  const r = instrument(generated, 'mod.ts', {
    coverageVariable: '__cov106',
    inputSourceMap,
    composeInputSourceMap: true,
  });
  const map = JSON.parse(r.coverageMap);

  // No counter reference in the code may point at a missing branch slot.
  const bRefs = [...new Set([...r.code.matchAll(/\.b\[(\d+)\]/g)].map((m) => m[1]))];
  const dangling = bRefs.filter((id) => !(id in map.b));
  assert.deepEqual(dangling, [], `emitted code must have no dangling branch counters, got ${dangling}`);
  assert.equal(Object.keys(map.branchMap).length, 0, 'the unmapped ternary branch must not be instrumented');

  // Execute the instrumented code: it must NOT throw.
  const sharedGlobal = {};
  assert.doesNotThrow(() => {
    new Function('globalThis', r.code)(sharedGlobal);
  }, 'instrumented eager-composed code must run without TypeError (issue #106)');
  assert.equal(sharedGlobal.__ran, 1 + 2 + 3 + 3, 'the program logic still executes correctly');

  // The mapped statement counters incremented (numbers, not NaN/undefined).
  const fc = sharedGlobal.__cov106['original.ts'];
  for (const [id, count] of Object.entries(fc.s)) {
    assert.ok(Number.isFinite(count), `statement counter ${id} must be a finite number, got ${count}`);
  }
  console.log('  [PASS] composeInputSourceMap-emitted code runs without crashing (issue #106)');
}

// Test: positive control - when the branch line maps, the branch is kept,
// the emitted code runs, and both arms are executable/countable.
{
  const generated = "const pick = (x) => (x > 0 ? 'pos' : 'neg');\nglobalThis.__r = pick(1) + pick(-1);\n";
  const inputSourceMap = JSON.stringify({
    version: 3,
    sources: ['original.ts'],
    names: [],
    mappings: 'AAAA;AACA',
    sourcesContent: ["const pick = (x) => (x > 0 ? 'pos' : 'neg');\nglobalThis.__r = pick(1) + pick(-1);\n"],
  });
  const r = instrument(generated, 'mod.ts', {
    coverageVariable: '__cov106b',
    inputSourceMap,
    composeInputSourceMap: true,
  });
  const map = JSON.parse(r.coverageMap);
  assert.equal(Object.keys(map.branchMap).length, 1, 'mapped branch is kept');
  assert.equal(map.branchMap['0'].locations.length, 2, 'both ternary arms survive when the line maps');

  const sharedGlobal = {};
  assert.doesNotThrow(() => {
    new Function('globalThis', r.code)(sharedGlobal);
  }, 'mapped-branch eager-composed code must run');
  assert.equal(sharedGlobal.__r, 'posneg', 'both arms executed');
  const fc = sharedGlobal.__cov106b['original.ts'];
  // Each arm taken once: b[0] = [1, 1].
  assert.deepEqual(fc.b['0'], [1, 1], 'both branch arms counted exactly once');
  console.log('  [PASS] composeInputSourceMap keeps mapped branch runnable (issue #106 control)');
}

// Test: composeInputSourceMap keeps entries whose generated position sits
// just before their line's first mapping, matching dropUnmapped (issue #122).
{
  // Two statements, line 2's statement starts at column 0 but that line's only
  // mapping is at column 2. A greatest-lower-bound-only gate would drop the
  // line-2 statement; the deferred dropUnmapped path keeps it (getMapping's
  // least-upper-bound fallback), so the eager path must agree.
  const generated = 'f();\ng();\n';
  const inputSourceMap = JSON.stringify({
    version: 3,
    sources: ['orig.js'],
    names: [],
    mappings: 'AASA;EAUA', // gen 1:0 -> orig 10:0 ; gen 2:2 -> orig 20:0
  });

  const eager = JSON.parse(
    instrument(generated, 'orig.js', {
      inputSourceMap,
      composeInputSourceMap: true,
      coverageVariable: '__cov122e',
    }).coverageMap,
  );

  const plain = JSON.parse(
    instrument(generated, 'orig.js', {
      inputSourceMap,
      coverageVariable: '__cov122d',
    }).coverageMap,
  );
  const deferred = JSON.parse(
    remapCoverageMap(JSON.stringify({ [plain.path]: plain }), { dropUnmapped: true }),
  )[plain.path];

  assert.equal(
    Object.keys(deferred.statementMap).length,
    2,
    'deferred dropUnmapped keeps both statements',
  );
  assert.equal(
    Object.keys(eager.statementMap).length,
    2,
    'eager compose keeps the col-0 statement whose line maps at col 2 (issue #122)',
  );
  assert.deepEqual(
    eager,
    deferred,
    'eager compose equals instrument-then-remap with dropUnmapped',
  );
  console.log('  [PASS] composeInputSourceMap keeps entries dropUnmapped keeps (issue #122)');
}

// Test: ignoreClassMethods drops fnMap entries for matching methods
{
  const source = `
    class C {
      keep() { return 1; }
      drop() { return 2; }
    }
  `;

  const baseline = instrument(source, 'class.js');
  const baselineNames = Object.values(JSON.parse(baseline.coverageMap).fnMap).map((f) => f.name);
  assert(baselineNames.includes('keep') && baselineNames.includes('drop'), 'Baseline should include both methods');

  const filtered = instrument(source, 'class.js', { ignoreClassMethods: ['drop'] });
  const filteredNames = Object.values(JSON.parse(filtered.coverageMap).fnMap).map((f) => f.name);
  assert(filteredNames.includes('keep'), 'Non-ignored method should still be tracked');
  assert(!filteredNames.includes('drop'), 'Ignored class method should be omitted from fnMap');
  console.log('  [PASS] ignoreClassMethods drops fnMap entries');
}

// Test: unhandledPragmas surfaces unrecognized istanbul/v8 directives
{
  const source = '/* istanbul ignore bogus */ const x = 1;';
  const result = instrument(source, 'pragma.js');
  assert(Array.isArray(result.unhandledPragmas), 'unhandledPragmas must be an array');
  assert.equal(result.unhandledPragmas.length, 1, 'Should report exactly one unhandled pragma');
  const [pragma] = result.unhandledPragmas;
  assert(pragma.comment.includes('istanbul ignore bogus'), 'Comment text should include the directive');
  assert.equal(pragma.line, 1, 'Line should be 1-based');
  assert.equal(pragma.column, 0, 'Column should be 0-based');

  const clean = instrument('const x = 1;', 'clean.js');
  assert.equal(clean.unhandledPragmas.length, 0, 'Clean source should have no unhandled pragmas');
  console.log('  [PASS] unhandledPragmas surfaces unknown directives');
}

// Test: remapCoverageMap rewrites coverage paths and positions through inputSourceMap
{
  const originalTs = 'const x: number = 1;\nconst y: number = 2;\nconst z: number = 3;\n';
  const intermediateJs = 'const x = 1;\nconst y = 2;\nconst z = 3;\n';
  const inputSourceMap = {
    version: 3,
    sources: ['src/app.ts'],
    sourcesContent: [originalTs],
    mappings: 'AAAA;AACA;AACA',
    names: [],
  };
  const result = instrument(intermediateJs, 'intermediate.js', {
    inputSourceMap: JSON.stringify(inputSourceMap),
  });

  const coverageMap = { 'intermediate.js': JSON.parse(result.coverageMap) };
  const remapped = JSON.parse(remapCoverageMap(JSON.stringify(coverageMap)));

  assert(remapped['src/app.ts'], 'remapped map should be keyed by the resolved original path');
  assert(!remapped['intermediate.js'], 'remapped map should not retain the intermediate key');
  assert.equal(remapped['src/app.ts'].path, 'src/app.ts', 'path on the file coverage should match');
  assert(!remapped['src/app.ts'].inputSourceMap, 'inputSourceMap should be cleared after remap');
  const lines = Object.values(remapped['src/app.ts'].statementMap)
    .map((loc) => loc.start.line)
    .sort();
  assert.deepEqual(lines, [1, 2, 3], 'statement lines should map back to original.ts lines');
  console.log('  [PASS] remapCoverageMap rewrites coverage through inputSourceMap');
}

// Multi-source fan-out and same-source collision parity with Istanbul.
{
  const { createSourceMapStore } = (await import('istanbul-lib-source-maps')).default;
  const { GenMapping, addMapping, setSourceContent, toEncodedMap } = await import(
    '@jridgewell/gen-mapping'
  );

  const sourceContent = '0123456789\n0123456789\n0123456789\n';
  const makeMap = (file, mappings) => {
    const map = new GenMapping({ file });
    for (const [generatedLine, source, originalLine] of mappings) {
      addMapping(map, {
        generated: { line: generatedLine, column: 0 },
        source,
        original: { line: originalLine, column: 0 },
      });
      setSourceContent(map, source, sourceContent);
    }
    return toEncodedMap(map);
  };
  const makeLoc = (line, start, end) => ({
    start: { line, column: start },
    end: { line, column: end },
  });
  const makeCoverage = (path, inputSourceMap, countBase) => {
    const coverage = {
      path,
      statementMap: {},
      fnMap: {},
      branchMap: {},
      s: {},
      f: {},
      b: {},
      bT: {},
      inputSourceMap,
      x_fallow_functionMap: {},
    };
    for (let line = 1; line <= 2; line += 1) {
      const id = String(line - 1);
      const loc = makeLoc(line, 0, 5);
      coverage.statementMap[id] = loc;
      coverage.fnMap[id] = { name: `f${line}`, decl: makeLoc(line, 0, 1), loc, line };
      coverage.branchMap[id] = {
        type: 'if',
        loc,
        line,
        locations: [makeLoc(line, 0, 2), makeLoc(line, 3, 5)],
      };
      coverage.s[id] = countBase + line;
      coverage.f[id] = countBase + line + 10;
      coverage.b[id] = [countBase + line, countBase + line + 1];
      coverage.bT[id] = [countBase + line + 2, countBase + line + 3];
      coverage.x_fallow_functionMap[id] = {
        id: `fallow:fn:${line.toString(16).padStart(8, '0')}`,
        name: `f${line}`,
        path: 'src/identity.ts',
        decl: makeLoc(line, 0, 1),
        loc,
      };
    }
    return coverage;
  };
  const endColumn = (column, line) =>
    Number.isFinite(column) ? column : (sourceContent.split('\n')[line - 1] ?? '').length;
  const locKey = (loc) =>
    `${loc.start.line}:${loc.start.column}:${loc.end.line}:${endColumn(
      loc.end.column,
      loc.end.line,
    )}`;
  const semantics = (coverage) => ({
    statements: Object.entries(coverage.statementMap)
      .map(([id, loc]) => `${locKey(loc)}=${coverage.s[id]}`)
      .sort(),
    functions: Object.entries(coverage.fnMap)
      .map(([id, fn]) => `${fn.name}|${locKey(fn.decl)}|${locKey(fn.loc)}=${coverage.f[id]}`)
      .sort(),
    branches: Object.entries(coverage.branchMap)
      .map(
        ([id, branch]) =>
          `${branch.type}|${locKey(branch.loc)}|${branch.locations.map(locKey).join(',')}=${coverage.b[
            id
          ].join(',')}`,
      )
      .sort(),
  });
  const findOracleFile = (coverageMap, suffix) => {
    const file = coverageMap.files().find((candidate) => candidate.endsWith(suffix));
    assert(file, `Istanbul oracle must contain ${suffix}`);
    return coverageMap.fileCoverageFor(file).data;
  };
  const assertFinalIdAlignment = (coverage) => {
    assert.deepEqual(Object.keys(coverage.s), Object.keys(coverage.statementMap));
    assert.deepEqual(Object.keys(coverage.f), Object.keys(coverage.fnMap));
    assert.deepEqual(Object.keys(coverage.b), Object.keys(coverage.branchMap));
    assert.deepEqual(Object.keys(coverage.bT), Object.keys(coverage.branchMap));
    if (coverage.x_fallow_functionMap) {
      assert.deepEqual(Object.keys(coverage.x_fallow_functionMap), Object.keys(coverage.fnMap));
    }
  };

  const bundle = makeCoverage(
    'dist/bundle.js',
    makeMap('dist/bundle.js', [
      [1, 'src/a.ts', 1],
      [2, 'src/b.ts', 1],
    ]),
    0,
  );
  const bundleInput = { [bundle.path]: bundle };
  const bundleOracle = await createSourceMapStore().transformCoverage(
    createCoverageMap(bundleInput),
  );
  const bundleOurs = JSON.parse(
    remapCoverageMap(JSON.stringify(bundleInput), { dropUnmapped: true }),
  );
  for (const source of ['src/a.ts', 'src/b.ts']) {
    assert(bundleOurs[source], `remapped bundle must retain ${source}`);
    assert.deepEqual(
      semantics(bundleOurs[source]),
      semantics(findOracleFile(bundleOracle, source)),
      `${source} semantics match Istanbul`,
    );
    assert.equal(Object.keys(bundleOurs[source].bT).length, 1, `${source} keeps bT`);
    assert.equal(
      Object.keys(bundleOurs[source].x_fallow_functionMap).length,
      1,
      `${source} keeps the function overlay`,
    );
    assertFinalIdAlignment(bundleOurs[source]);
  }
  assert.deepEqual(
    bundleOurs['src/a.ts'].x_fallow_functionMap['0'],
    bundle.x_fallow_functionMap['0'],
  );
  assert.deepEqual(
    bundleOurs['src/b.ts'].x_fallow_functionMap['0'],
    bundle.x_fallow_functionMap['1'],
  );

  const first = makeCoverage(
    'dist/first.js',
    makeMap('dist/first.js', [
      [1, 'src/shared.ts', 1],
      [2, 'src/shared.ts', 2],
    ]),
    0,
  );
  const second = makeCoverage(
    'dist/second.js',
    makeMap('dist/second.js', [
      [1, 'src/shared.ts', 1],
      [2, 'src/shared.ts', 3],
    ]),
    20,
  );
  second.fnMap['0'].name = 'different';
  second.fnMap['0'].loc = makeLoc(2, 0, 5);
  second.branchMap['0'].type = 'cond-expr';
  second.branchMap['0'].loc = makeLoc(2, 0, 5);
  const collisionInput = { [first.path]: first, [second.path]: second };
  const collisionOracle = await createSourceMapStore().transformCoverage(
    createCoverageMap(collisionInput),
  );
  const collisionOurs = JSON.parse(
    remapCoverageMap(JSON.stringify(collisionInput), { dropUnmapped: true }),
  );
  assert.deepEqual(
    semantics(collisionOurs['src/shared.ts']),
    semantics(findOracleFile(collisionOracle, 'src/shared.ts')),
    'collision locations and counts match Istanbul',
  );
  const sharedBranchId = Object.entries(collisionOurs['src/shared.ts'].branchMap).find(
    ([, branch]) => branch.loc.start.line === 1,
  )[0];
  assert.deepEqual(collisionOurs['src/shared.ts'].bT[sharedBranchId], [26, 28], 'bT sums by arm');
  assertFinalIdAlignment(collisionOurs['src/shared.ts']);
  assert.deepEqual(
    collisionOurs['src/shared.ts'].x_fallow_functionMap['0'],
    first.x_fallow_functionMap['0'],
  );

  const conflicting = structuredClone(second);
  conflicting.x_fallow_functionMap['0'].id = 'fallow:fn:conflict';
  const conflictInput = { [first.path]: first, [conflicting.path]: conflicting };
  const conflictOurs = JSON.parse(
    remapCoverageMap(JSON.stringify(conflictInput), { dropUnmapped: true }),
  );
  assert.equal(
    conflictOurs['src/shared.ts'].x_fallow_functionMap,
    undefined,
    'conflicting overlay records drop the optional overlay',
  );
  assertFinalIdAlignment(conflictOurs['src/shared.ts']);

  const statementOnly = structuredClone(second);
  statementOnly.fnMap = {};
  statementOnly.f = {};
  statementOnly.x_fallow_functionMap = undefined;
  const statementOnlyInput = { [first.path]: first, [statementOnly.path]: statementOnly };
  const statementOnlyOurs = JSON.parse(
    remapCoverageMap(JSON.stringify(statementOnlyInput), { dropUnmapped: true }),
  );
  assertFinalIdAlignment(statementOnlyOurs['src/shared.ts']);
  assert.deepEqual(
    statementOnlyOurs['src/shared.ts'].x_fallow_functionMap['0'],
    first.x_fallow_functionMap['0'],
    'statement-only collision preserves a complete overlay record',
  );

  const crossSource = makeCoverage(
    'dist/cross-source.js',
    makeMap('dist/cross-source.js', [
      [1, 'src/a.ts', 1],
      [2, 'src/b.ts', 1],
    ]),
    0,
  );
  crossSource.statementMap = {};
  crossSource.fnMap = {};
  crossSource.s = {};
  crossSource.f = {};
  crossSource.x_fallow_functionMap = {};
  crossSource.branchMap = {
    0: {
      type: 'if',
      loc: makeLoc(1, 0, 5),
      line: 1,
      locations: [makeLoc(1, 0, 2), makeLoc(2, 0, 2)],
    },
  };
  crossSource.b = { 0: [1, 2] };
  crossSource.bT = { 0: [3, 4] };
  const crossSourceInput = { [crossSource.path]: crossSource };
  const crossSourceOracle = await createSourceMapStore().transformCoverage(
    createCoverageMap(crossSourceInput),
  );
  const crossSourceOurs = JSON.parse(
    remapCoverageMap(JSON.stringify(crossSourceInput), { dropUnmapped: true }),
  );
  assert.deepEqual(
    Object.keys(crossSourceOurs),
    crossSourceOracle.files(),
    'cross-source branch arms are dropped exactly like Istanbul',
  );

  console.log('  [PASS] remapCoverageMap fans out and merges like Istanbul');
}

// Test: v8ToIstanbul converts V8 UTF-16 range coverage into Istanbul FileCoverage
{
  const source = 'const x = 1;\nconst y = 2;\nconst z = 3;\n';
  const functions = [
    {
      functionName: '',
      ranges: [{ startOffset: 0, endOffset: source.length, count: 1 }],
      isBlockCoverage: false,
    },
  ];
  const fc = JSON.parse(v8ToIstanbul(source, 'app.js', JSON.stringify(functions)));
  const counts = Object.values(fc.s);
  assert(counts.length > 0, 'statementMap should populate');
  assert(counts.every((c) => c === 1), `all statements covered, got: ${JSON.stringify(counts)}`);
  console.log('  [PASS] v8ToIstanbul produces Istanbul FileCoverage from V8 ranges');
}

// Test: remapCoverageMapWithLoader supplies maps for entries without inputSourceMap
{
  const intermediateJs = 'const x = 1;\nconst y = 2;\nconst z = 3;\n';
  const inputSourceMap = JSON.stringify({
    version: 3,
    sources: ['src/app.ts'],
    sourcesContent: ['const x: number = 1;\nconst y: number = 2;\nconst z: number = 3;\n'],
    mappings: 'AAAA;AACA;AACA',
    names: [],
  });
  // Instrument WITHOUT inputSourceMap so the coverage has none embedded.
  const result = instrument(intermediateJs, 'intermediate.js');
  const coverageMap = { 'intermediate.js': JSON.parse(result.coverageMap) };

  // Loader-supplied path: the dictionary key matches the FileCoverage path.
  const remapped = JSON.parse(
    remapCoverageMapWithLoader(JSON.stringify(coverageMap), { 'intermediate.js': inputSourceMap }),
  );
  assert(remapped['src/app.ts'], 'loader-supplied map should remap the entry');
  assert(!remapped['intermediate.js'], 'intermediate key should be replaced');

  // Empty dictionary: passthrough.
  const passthrough = JSON.parse(remapCoverageMapWithLoader(JSON.stringify(coverageMap), {}));
  assert(passthrough['intermediate.js'], 'no loader entry -> passthrough');
  console.log('  [PASS] remapCoverageMapWithLoader supplies external maps');
}

// Test: v8ToIstanbulWithLoader resolves external //# sourceMappingURL references
{
  const map = JSON.stringify({
    version: 3,
    sources: ['src/app.ts'],
    sourcesContent: ['const x: number = 1;\n'],
    mappings: 'AAAA',
    names: [],
  });
  const source = 'const x = 1;\n//# sourceMappingURL=app.js.map\n';
  const functions = [
    {
      functionName: '',
      ranges: [{ startOffset: 0, endOffset: source.length, count: 1 }],
      isBlockCoverage: false,
    },
  ];

  const fc = JSON.parse(
    v8ToIstanbulWithLoader(source, 'app.js', JSON.stringify(functions), { 'app.js.map': map }),
  );
  assert(fc.inputSourceMap, 'loader entry should be attached as inputSourceMap');
  assert.equal(fc.inputSourceMap.sources[0], 'src/app.ts');

  // No matching loader entry -> map left unset.
  const fc2 = JSON.parse(v8ToIstanbulWithLoader(source, 'app.js', JSON.stringify(functions), {}));
  assert(!fc2.inputSourceMap, 'unknown URL -> inputSourceMap unset');
  console.log('  [PASS] v8ToIstanbulWithLoader resolves external map URLs');
}

{
  const source = 'const marker = `\n//# sourceMappingURL=ghost.map\n`;\n';
  const functions = [{
    functionName: '',
    ranges: [{ startOffset: 0, endOffset: source.length, count: 1 }],
    isBlockCoverage: false,
  }];
  const map = JSON.stringify({
    version: 3,
    sources: ['ghost.ts'],
    mappings: 'AAAA',
    names: [],
  });
  const coverage = JSON.parse(
    v8ToIstanbulWithLoader(source, 'app.js', JSON.stringify(functions), { 'ghost.map': map }),
  );
  assert(!coverage.inputSourceMap, 'template marker must not load an external source map');
  console.log('  [PASS] sourceMappingURL requires a final directive line');
}

// Test: v8ToIstanbul resolves if-arm[0] through the collected then-block span
{
  const source = 'function f(x) {\n  if (x) {\n    a();\n  } else {\n    b();\n  }\n}\n';
  const moduleEnd = source.length;
  const thenStart = source.indexOf('if (x) {') + 7;
  const thenEnd = source.indexOf('} else') + 1;
  const elseStart = source.indexOf('else {') + 5;
  const elseEnd = source.lastIndexOf('\n  }') + 4;

  // Function ran 5 times; predicate truthy 3 of those; else taken 2 of those.
  const functions = [
    {
      functionName: 'f',
      ranges: [
        { startOffset: 0, endOffset: moduleEnd, count: 5 },
        { startOffset: thenStart, endOffset: thenEnd, count: 3 },
        { startOffset: elseStart, endOffset: elseEnd, count: 2 },
      ],
      isBlockCoverage: true,
    },
  ];
  const fc = JSON.parse(v8ToIstanbul(source, 'ifelse.js', JSON.stringify(functions)));
  const [ifId] = Object.entries(fc.branchMap).find(([, entry]) => entry.type === 'if');
  assert.deepEqual(
    fc.b[ifId],
    [3, 2],
    'arm[0] now reflects then-block count; arm[1] reflects else-block count',
  );
  console.log('  [PASS] v8ToIstanbul resolves if-arm[0] through collected body span');
}

// Test: stripTypescript option strips TS annotations and produces executable JS
{
  const result = instrument(
    'const x: number = 1;\nconsole.log(x);\n',
    'app.ts',
    { stripTypescript: true, sourceMap: true },
  );
  assert(!result.code.includes(': number'), 'TS annotation must be stripped');
  assert(result.code.includes('const x ='), 'output must contain executable JS');
  const cov = JSON.parse(result.coverageMap);
  assert(cov.path === 'app.ts', 'coverage map path should be app.ts');
  assert(Object.keys(cov.statementMap).length >= 2, 'statementMap should be populated');
  console.log('  [PASS] stripTypescript option strips TS and produces executable JS');
}

// Test: stripTypescript defaults to false (preserves backward compatibility)
{
  const result = instrument('const x: number = 1;\n', 'app.ts');
  assert(result.code.includes(': number'), 'without stripTypescript the TS annotation must remain');
  console.log('  [PASS] stripTypescript defaults to false');
}

// Test: explicit sourceType overrides host id inference
{
  const result = instrument('const value: number = 1;\n', '/src/a.ts?raw', {
    sourceType: 'ts',
    stripTypescript: true,
  });
  assert(!result.code.includes(': number'), 'explicit ts sourceType must parse TypeScript');
  console.log('  [PASS] explicit sourceType overrides host id inference');
}

// Test: unknown instrument option keys fail loudly
{
  assert.throws(
    () => instrument('const value = 1;\n', 'unknown-option.js', { trackOptionalChain: false }),
    (error) => {
      assert.match(error.message, /trackOptionalChain/);
      assert.match(error.message, /trackOptionalChainBranches/);
      return true;
    },
  );
  console.log('  [PASS] unknown instrument option keys fail loudly');
}

// Test: Istanbul compatibility profile switches every representable divergence
{
  const result = instrument(
    'let x; x ??= 1; const f = function () {}; class C { method() {} } const o = { execute() {} }; const y = o?.value; if (y) work();',
    'compat.js',
    { compat: 'istanbul' },
  );
  const coverage = JSON.parse(result.coverageMap);
  const functions = Object.values(coverage.fnMap);

  assert.deepEqual(
    functions.map((entry) => entry.name),
    ['(anonymous_0)', '(anonymous_1)', '(anonymous_2)'],
  );
  assert(functions.slice(1).every((entry) => entry.decl.end.column === entry.decl.start.column + 1));
  assert(!Object.values(coverage.branchMap).some((entry) => entry.type === 'binary-expr'));
  assert(!Object.values(coverage.branchMap).some((entry) => entry.type === 'optional-chain'));
  const ifEntry = Object.values(coverage.branchMap).find((entry) => entry.type === 'if');
  assert.deepEqual(ifEntry.locations[1], { start: {}, end: {} });
  console.log('  [PASS] Istanbul compatibility profile switches coverage shape');
}

// Test: compatibility profiles reject unknown values
{
  assert.throws(
    () => instrument('const value = 1;', 'compat.js', { compat: 'other' }),
    /unknown variant `other`.*`istanbul`/,
  );
  assert.throws(() => createOxcInstrumenter({ compat: 'other' }), /compat.*'istanbul'/);
  console.log('  [PASS] compatibility profiles reject unknown values');
}

// Test: createOxcInstrumenter auto-detects .ts as raw TypeScript when no inputSourceMap
{
  const inst = createOxcInstrumenter();
  const code = inst.instrumentSync('const x: number = 1;\nconsole.log(x);\n', 'app.ts');
  assert(!code.includes(': number'), 'auto-detect must strip TS on .ts without inputSourceMap');
  assert(code.includes('const x ='), 'auto-detect must emit executable JS');
  console.log('  [PASS] vitest adapter auto-detects .ts without inputSourceMap');
}

// Test: createOxcInstrumenter auto-detects .tsx and preserves JSX
{
  const inst = createOxcInstrumenter();
  const code = inst.instrumentSync(
    'const el: JSX.Element = <div>hi</div>;\nconsole.log(el);\n',
    'app.tsx',
  );
  assert(!code.includes(': JSX.Element'), 'auto-detect must strip TS on .tsx');
  assert(code.includes('<div>'), 'auto-detect must preserve JSX on .tsx');
  console.log('  [PASS] vitest adapter auto-detects .tsx and preserves JSX');
}

// Test: createOxcInstrumenter does NOT auto-strip when inputSourceMap is provided
{
  const inst = createOxcInstrumenter();
  // Pass raw TS source WITH an inputSourceMap. In real Vite/Vitest usage the
  // source would already be transformed JS by this point, but feeding raw TS
  // is the cleanest way to OBSERVE whether the strip pass ran. Auto-detect
  // must treat the inputSourceMap presence as "already transformed upstream"
  // and skip the strip; the TS annotation in the output proves it.
  const fakeMap = { version: 3, sources: ['orig.ts'], mappings: '', names: [] };
  const code = inst.instrumentSync('const x: number = 1;\n', 'app.ts', fakeMap);
  assert(
    code.includes(': number'),
    'with inputSourceMap the strip pass must not run, TS annotation must survive',
  );
  console.log('  [PASS] vitest adapter does not strip when inputSourceMap is present');
}

// Test: createOxcInstrumenter does NOT auto-strip .js files
{
  const inst = createOxcInstrumenter();
  const code = inst.instrumentSync('const x = 1;\nconsole.log(x);\n', 'app.js');
  assert(code.includes('const x ='), '.js must pass through as executable JS');
  console.log('  [PASS] vitest adapter does not auto-strip .js files');
}

// Test: the call sequence of vite-plugin-istanbul 9. @storybook/addon-coverage
// gives `istanbul.instrumenter` to this plugin on the Vite builder. For each
// file, the plugin calls instrumentSync() with the combined Vite source map
// and serves that code. Then it calls instrumentSync() again with an identity
// map, and reads lastSourceMap() and the `fileCoverage` property.
{
  const { GenMapping, addMapping, setSourceContent, toEncodedMap } = await import(
    '@jridgewell/gen-mapping'
  );
  const vm = await import('node:vm');
  const filename = '/project/src/add.ts';
  const original = 'export function add(a: number, b: number) {\n  return a > 0 ? a + b : b;\n}\nadd(1, 2);\n';
  const compiled = 'function add(a, b) {\n  return a > 0 ? a + b : b;\n}\nadd(1, 2);\n';
  const lineMap = (sourceContent) => {
    const map = new GenMapping({ file: filename });
    const lines = compiled.split('\n').length - 1;
    for (let line = 1; line <= lines; line++) {
      addMapping(map, {
        generated: { line, column: 0 },
        source: filename,
        original: { line, column: 0 },
      });
    }
    setSourceContent(map, filename, sourceContent);
    return toEncodedMap(map);
  };
  const combinedMap = lineMap(original);
  const identityMap = lineMap(compiled);

  const runPluginSequence = (instrumenter) => {
    const code = instrumenter.instrumentSync(compiled, filename, combinedMap);
    instrumenter.instrumentSync(compiled, filename, identityMap);
    return { code, map: instrumenter.lastSourceMap(), fileCoverage: instrumenter.fileCoverage };
  };
  const shape = (coverage) => ({
    statementMap: coverage.statementMap,
    fnMap: coverage.fnMap,
    branchMap: coverage.branchMap,
  });

  const oxc = runPluginSequence(createOxcInstrumenter({ compat: 'istanbul' }));
  const reference = runPluginSequence(
    createInstrumenter({
      coverageGlobalScopeFunc: false,
      coverageGlobalScope: 'globalThis',
      preserveComments: true,
      produceSourceMap: true,
      autoWrap: true,
      esModules: true,
      compact: false,
    }),
  );

  assert.equal(oxc.map?.version, 3, 'lastSourceMap() must return the map of the second call');
  assert.equal(oxc.fileCoverage.path, filename, 'fileCoverage must describe the last file');
  assert.deepEqual(shape(oxc.fileCoverage), shape(reference.fileCoverage), 'fileCoverage maps must match Istanbul');

  const runCounters = (code) => {
    const context = vm.createContext({});
    vm.runInContext(code, context);
    // JSON copy: objects from another vm context fail a strict deepEqual.
    return JSON.parse(JSON.stringify(context.__coverage__[filename]));
  };
  const served = runCounters(oxc.code);
  const servedReference = runCounters(reference.code);
  assert.deepEqual(shape(served), shape(servedReference), 'served code must register the Istanbul maps');
  assert.deepEqual(
    { s: served.s, f: served.f, b: served.b },
    { s: servedReference.s, f: servedReference.f, b: servedReference.b },
    'served code must count the same hits as Istanbul',
  );
  assert.deepEqual(
    served.inputSourceMap?.sources,
    combinedMap.sources,
    'served code must keep the combined Vite map for the remap step',
  );
  console.log('  [PASS] vitest adapter follows the vite-plugin-istanbul 9 call sequence');
}

// Test: explicit stripTypescript: false overrides auto-detect on .ts
{
  const inst = createOxcInstrumenter({ stripTypescript: false });
  const code = inst.instrumentSync('const x: number = 1;\n', 'app.ts');
  assert(
    code.includes(': number'),
    'explicit stripTypescript: false must keep TS annotations on .ts',
  );
  console.log('  [PASS] vitest adapter honors explicit stripTypescript: false on .ts');
}

// Test: explicit stripTypescript: true overrides the auto-detect skip on .ts
// when inputSourceMap is present. Auto-detect would NOT strip (inputSourceMap
// signals "already transformed upstream"), but explicit true forces the strip
// pass anyway. Observable via the TS annotation disappearing from the output.
{
  const inst = createOxcInstrumenter({ stripTypescript: true });
  const fakeMap = { version: 3, sources: ['orig.ts'], mappings: '', names: [] };
  const code = inst.instrumentSync('const x: number = 1;\n', 'app.ts', fakeMap);
  assert(
    !code.includes(': number'),
    'explicit stripTypescript: true must force strip even when inputSourceMap is present',
  );
  console.log('  [PASS] vitest adapter honors explicit stripTypescript: true override');
}

// Test: non-boolean stripTypescript throws TypeError instead of silently
// coercing. Catches the 'auto' string case (a prior tri-state design shape)
// that Boolean coercion would turn into force-strip.
{
  let caught = null;
  try {
    createOxcInstrumenter({ stripTypescript: 'auto' });
  } catch (e) {
    caught = e;
  }
  assert(caught instanceof TypeError, `expected TypeError, got ${caught}`);
  assert(
    caught.message.includes('stripTypescript'),
    `error message must name the offending option, got: ${caught.message}`,
  );
  console.log('  [PASS] vitest adapter rejects non-boolean stripTypescript with TypeError');
}

// Test: TS_EXTENSION_REGEX rejects non-TypeScript extensions that look similar.
// The narrow form /\.([mc]ts|tsx?)$/i must not match .ts.bak (where .bak is
// the actual extension) or .mtsx (not a real Node / TS extension).
// Strategy: feed TS source to a non-matching filename. Auto-detect must skip
// the strip; the parser then rejects the TS syntax because the filename
// extension (.bak / .mtsx) means SourceType::from_path falls back to JS.
{
  const inst = createOxcInstrumenter();
  // .ts.bak: actual extension is .bak; strip must not auto-engage.
  let caughtBak = null;
  try {
    inst.instrumentSync('const x: number = 1;\n', 'app.ts.bak');
  } catch (e) {
    caughtBak = e;
  }
  assert(
    caughtBak !== null && /parse error/.test(caughtBak.message),
    'app.ts.bak must not auto-strip; parser must reject TS syntax',
  );
  // .mtsx: not a real extension (only .mts / .cts / .ts / .tsx are valid).
  let caughtMtsx = null;
  try {
    inst.instrumentSync('const x: number = 1;\n', 'app.mtsx');
  } catch (e) {
    caughtMtsx = e;
  }
  assert(
    caughtMtsx !== null && /parse error/.test(caughtMtsx.message),
    'app.mtsx must not auto-strip; .mtsx is not a real TypeScript extension',
  );
  console.log(
    '  [PASS] vitest adapter regex rejects .ts.bak and .mtsx (non-TS extensions)',
  );
}

// Test: createOxcInstrumenter auto-promotes emitDecoratorMetadata to
// experimentalDecorators (tsconfig.json semantics). The bare napi instrument()
// rejects the same combination, but the vitest adapter normalizes it locally
// so vitest+NestJS users with only emitDecoratorMetadata configured do not
// see a runtime throw on the first instrumented file.
{
  const inst = createOxcInstrumenter({ emitDecoratorMetadata: true });
  const code = inst.instrumentSync(
    "@Injectable() export class S { constructor(private readonly b: B) {} }\n",
    's.ts',
  );
  assert(
    code.includes('_decorate('),
    'auto-promotion must produce _decorate call',
  );
  assert(
    code.includes('_decorateMetadata('),
    'auto-promotion must still emit metadata calls',
  );
  console.log('  [PASS] vitest adapter auto-promotes emitDecoratorMetadata');
}

// Test: experimentalDecorators + emitDecoratorMetadata lowers NestJS-style
// decorators to _decorate / _decorateMetadata calls importing helpers from
// @oxc-project/runtime. Mirrors the Rust-side
// ts_direct_decorator_metadata_emits_helper_imports test.
{
  const src =
    "import { Injectable, Body } from '@nestjs/common';\n" +
    '@Injectable()\n' +
    'export class FooService {\n' +
    '  constructor(private readonly bar: BarRepo) {}\n' +
    '  method(@Body() dto: CreateDto): string {\n' +
    '    return dto.name;\n' +
    '  }\n' +
    '}\n';
  const result = instrument(src, 'foo.service.ts', {
    stripTypescript: true,
    experimentalDecorators: true,
    emitDecoratorMetadata: true,
  });
  assert(
    result.code.includes('import _decorate from "@oxc-project/runtime/helpers/decorate"'),
    'expected @oxc-project/runtime _decorate import',
  );
  assert(
    result.code.includes('_decorateMetadata("design:paramtypes"'),
    'expected design:paramtypes metadata call',
  );
  console.log('  [PASS] napi adapter: experimentalDecorators + emitDecoratorMetadata');
}

// Test: emitDecoratorMetadata: true without experimentalDecorators: true is
// rejected with a JS Error rather than silently promoted. The Rust API uses
// a single DecoratorMode enum so the invalid combination is unrepresentable;
// the napi adapter mirrors that by rejecting at the boundary.
{
  const src =
    '@Injectable() export class S { constructor(private readonly b: B) {} }\n';
  let caught = null;
  try {
    instrument(src, 's.ts', {
      stripTypescript: true,
      emitDecoratorMetadata: true,
      // experimentalDecorators intentionally omitted.
    });
  } catch (e) {
    caught = e;
  }
  assert(
    caught !== null,
    'emitDecoratorMetadata without experimentalDecorators must throw',
  );
  assert(
    /experimentalDecorators/.test(caught.message),
    `error must explain the required flag, got: ${caught.message}`,
  );
  console.log('  [PASS] napi adapter: emitDecoratorMetadata without experimentalDecorators throws');
}

// Test: experimentalDecorators=true + emitDecoratorMetadata=false lowers
// decorators without emitting design:type / design:paramtypes metadata.
{
  const src =
    '@Injectable() export class S { constructor(private readonly b: B) {} }\n';
  const result = instrument(src, 's.ts', {
    stripTypescript: true,
    experimentalDecorators: true,
    emitDecoratorMetadata: false,
  });
  assert(
    result.code.includes('_decorate('),
    'experimentalDecorators alone must still produce _decorate call',
  );
  assert(
    !result.code.includes('_decorateMetadata('),
    'no metadata calls expected when emitDecoratorMetadata is false',
  );
  console.log('  [PASS] napi adapter: experimentalDecorators without emitDecoratorMetadata');
}

// Test: with both flags off (default), decorators flow through verbatim.
// Mirrors ts_direct_decorators_default_pass_through.
{
  const src = '@Injectable() export class S {}\n';
  const result = instrument(src, 's.ts', { stripTypescript: true });
  assert(
    result.code.includes('@Injectable()'),
    'decorator syntax must survive verbatim when both flags default to false',
  );
  assert(
    !result.code.includes('@oxc-project/runtime'),
    'no helper import expected when both decorator flags are off',
  );
  console.log('  [PASS] napi adapter: decorators pass through by default');
}

// Test: functionIdentityOverlay surfaces a stable fallow:fn:<hex> per
// function under the x_fallow_functionMap key on the parsed coverage map,
// keyed by the same ids as fnMap. Default (option off) must NOT include the
// key; standard Istanbul consumers see byte-identical shape.
{
  const src = 'function add(a, b) { return a + b; }\nfunction sub(a, b) { return a - b; }\n';

  const defaultResult = instrument(src, 'app.js');
  const defaultMap = JSON.parse(defaultResult.coverageMap);
  assert(
    defaultMap.x_fallow_functionMap === undefined,
    'default output must omit x_fallow_functionMap',
  );

  const overlayResult = instrument(src, 'app.js', { functionIdentityOverlay: true });
  const overlayMap = JSON.parse(overlayResult.coverageMap);
  const overlay = overlayMap.x_fallow_functionMap;
  assert(overlay !== undefined, 'overlay must be present when functionIdentityOverlay is true');
  const fnKeys = Object.keys(overlayMap.fnMap).sort();
  const overlayKeys = Object.keys(overlay).sort();
  assert(
    JSON.stringify(fnKeys) === JSON.stringify(overlayKeys),
    `overlay keys must align with fnMap keys, fnMap=${fnKeys} overlay=${overlayKeys}`,
  );
  for (const k of fnKeys) {
    assert(
      typeof overlay[k].id === 'string' && overlay[k].id.startsWith('fallow:fn:'),
      `overlay[${k}].id must be fallow:fn:<hex>, got ${JSON.stringify(overlay[k])}`,
    );
    assert(overlay[k].path === 'app.js', `overlay[${k}].path must mirror file path`);
  }

  // Determinism: re-instrument the same source, ids must match.
  const repeat = JSON.parse(
    instrument(src, 'app.js', { functionIdentityOverlay: true }).coverageMap,
  ).x_fallow_functionMap;
  for (const k of fnKeys) {
    assert(
      overlay[k].id === repeat[k].id,
      `id must be deterministic across runs; key ${k} drifted`,
    );
  }

  console.log('  [PASS] napi adapter: functionIdentityOverlay attaches x_fallow_functionMap');
}

// Test: createOxcInstrumenter forwards functionIdentityOverlay to the native
// instrumenter so Vitest users can opt into the same downstream identity data.
{
  const inst = createOxcInstrumenter({ functionIdentityOverlay: true });
  inst.instrumentSync('function handler() { return 1; }\n', 'app.js');
  const fc = inst.lastFileCoverage();
  assert(
    fc.x_fallow_functionMap !== undefined,
    'vitest adapter must forward functionIdentityOverlay to native instrumenter',
  );
  assert.deepEqual(Object.keys(fc.x_fallow_functionMap).sort(), Object.keys(fc.fnMap).sort());
  assert(
    fc.x_fallow_functionMap['0'].id.startsWith('fallow:fn:'),
    'vitest adapter overlay must carry fallow:fn:<hex> ids',
  );

  const defaultInst = createOxcInstrumenter();
  defaultInst.instrumentSync('function handler() { return 1; }\n', 'app.js');
  assert(
    defaultInst.lastFileCoverage().x_fallow_functionMap === undefined,
    'vitest adapter default output must omit x_fallow_functionMap',
  );

  console.log('  [PASS] vitest adapter forwards functionIdentityOverlay');
}

// Test: remapCoverageMap rejects a single-FileCoverage input with a hint
// pointing at the correct CoverageMap shape. The raw serde error reads
// "expected struct FileCoverage" when you pass one, which makes the user
// blame their FileCoverage shape rather than realising they handed over the
// wrong outer container.
{
  const r = instrument('function handler() { return 42; }\n', 'app.bundled.js', {
    inputSourceMap: JSON.stringify({
      version: 3,
      file: 'app.bundled.js',
      sources: ['src/app.ts'],
      sourcesContent: ['function handler(){return 42}'],
      names: [],
      mappings: '',
    }),
  });
  let caught = null;
  try {
    remapCoverageMap(r.coverageMap);
  } catch (e) {
    caught = e;
  }
  assert(caught !== null, 'remapCoverageMap must throw when given a single FileCoverage');
  assert(
    caught.message.includes('CoverageMap shape'),
    `error must hint at CoverageMap shape, got: ${caught.message}`,
  );
  assert(
    caught.message.includes('{[path]: FileCoverage}'),
    `error must spell out the expected shape, got: ${caught.message}`,
  );

  let loaderCaught = null;
  try {
    remapCoverageMapWithLoader(r.coverageMap, {});
  } catch (e) {
    loaderCaught = e;
  }
  assert(loaderCaught !== null, 'loader remap must also reject a single FileCoverage');
  assert(
    loaderCaught.message.includes('the remap API expects'),
    `loader error must use the shared remap API hint, got: ${loaderCaught.message}`,
  );
  assert(
    !loaderCaught.message.includes('remapCoverageMap expects'),
    `loader error must not name a different function, got: ${loaderCaught.message}`,
  );

  // Sanity: the same input wrapped in the correct shape works.
  const fc = JSON.parse(r.coverageMap);
  const remapped = JSON.parse(remapCoverageMap(JSON.stringify({ [fc.path]: fc })));
  assert(
    Object.keys(remapped).length > 0,
    'wrapped CoverageMap must remap to at least one file',
  );

  console.log('  [PASS] remapCoverageMap hints at wrong-shape input');
}

// Test: remapCoverageMap drop_unmapped prunes entries on unmapped lines (issue #92)
{
  // Single-line identity map: line 2 of the generated file maps to line 1 of
  // src/app.ts; line 1 is unmapped. Without `dropUnmapped`, unmapped positions
  // keep their generated-output coordinates (line 1); with
  // `dropUnmapped: true`, those entries (and their matching s/f/b slots) are
  // pruned from the FileCoverage, matching istanbul-lib-source-maps's
  // transformer.js behaviour.
  const inputSourceMap = {
    version: 3,
    sources: ['src/app.ts'],
    sourcesContent: ['const y: number = 2;\n'],
    mappings: ';AAAA',
    names: [],
  };
  const intermediateJs = 'const x = 1;\nconst y = 2;\n';
  const result = instrument(intermediateJs, 'intermediate.js', {
    inputSourceMap: JSON.stringify(inputSourceMap),
  });
  const originalCoverage = JSON.parse(result.coverageMap);
  const coverageMap = { 'intermediate.js': originalCoverage };
  const inputJson = JSON.stringify(coverageMap);
  assert.deepEqual(
    Object.keys(originalCoverage.statementMap),
    ['0', '1'],
    'fixture must contain the two generated statements before remapping',
  );
  assert.equal(
    originalCoverage.statementMap['0'].start.line,
    1,
    'original statement 0 must be the unmapped generated-line-1 entry',
  );
  assert.equal(
    originalCoverage.statementMap['1'].start.line,
    2,
    'original statement 1 must be the mapped generated-line-2 entry',
  );

  const kept = JSON.parse(remapCoverageMap(inputJson));
  const pruned = JSON.parse(remapCoverageMap(inputJson, { dropUnmapped: true }));

  const keptStatements = Object.keys(kept['src/app.ts'].statementMap).length;
  const prunedStatements = Object.keys(pruned['src/app.ts'].statementMap).length;
  assert(
    keptStatements > prunedStatements,
    `dropUnmapped must prune at least one statement (kept=${keptStatements}, pruned=${prunedStatements})`,
  );
  // Every surviving entry must sit on the mapped line.
  for (const loc of Object.values(pruned['src/app.ts'].statementMap)) {
    assert.equal(loc.start.line, 1, `surviving statement must be on the mapped line, got ${loc.start.line}`);
  }
  // `s` slot keys stay aligned with `statementMap` keys.
  assert.deepEqual(
    Object.keys(pruned['src/app.ts'].s).sort(),
    Object.keys(pruned['src/app.ts'].statementMap).sort(),
    'surviving `s` keys must match `statementMap` keys',
  );
  assert.deepEqual(
    Object.keys(pruned['src/app.ts'].statementMap),
    ['0'],
    'map-level remapping must renumber the surviving original statement 1 to contiguous ID 0',
  );

  // Default (omitted options) is unchanged.
  const defaultRemap = JSON.parse(remapCoverageMap(inputJson));
  assert.equal(
    Object.keys(defaultRemap['src/app.ts'].statementMap).length,
    keptStatements,
    'omitting options must behave identically to the prior API',
  );

  console.log('  [PASS] remapCoverageMap dropUnmapped prunes unmapped entries');
}

// Test: remapCoverageMapWithLoader respects dropUnmapped
{
  const inputSourceMap = JSON.stringify({
    version: 3,
    sources: ['src/app.ts'],
    sourcesContent: ['const x: number = 1;\n'],
    mappings: 'AAAA',
    names: [],
  });
  const intermediateJs = 'const x = 1;\nconst y = 2;\n';
  const result = instrument(intermediateJs, 'intermediate.js');
  const coverageMap = { 'intermediate.js': JSON.parse(result.coverageMap) };
  const inputJson = JSON.stringify(coverageMap);
  const loader = { 'intermediate.js': inputSourceMap };

  const kept = JSON.parse(remapCoverageMapWithLoader(inputJson, loader));
  const pruned = JSON.parse(remapCoverageMapWithLoader(inputJson, loader, { dropUnmapped: true }));

  const keptStatements = Object.keys(kept['src/app.ts'].statementMap).length;
  const prunedStatements = Object.keys(pruned['src/app.ts'].statementMap).length;
  assert(
    keptStatements > prunedStatements,
    `loader form: dropUnmapped must prune at least one statement (kept=${keptStatements}, pruned=${prunedStatements})`,
  );

  console.log('  [PASS] remapCoverageMapWithLoader respects dropUnmapped');
}

// Test: remapCoverageMap drops a pre-existing orphan counter so the result
// merges cleanly in istanbul-lib-coverage / nyc (issue #107). An orphan is an
// `s`/`f`/`b` key with no matching location-map entry; it crashes
// `CoverageMap.merge` with "Cannot destructure property 'start' of 'undefined'".
// Such orphans can reach a coverage object at runtime when an upstream
// instrumenter incremented `cov.s[id]` against a slot later pruned from the map
// (NaN serializes back as `null`, re-ingested as an orphan key).
{

  // The exact malformed shape from the issue: statementMap is missing "1", but
  // s has key "1" (value null, the NaN a dangling `++cov.s[1]` produced).
  const orphanFile = () => ({
    path: '/x/mod.ts',
    statementMap: { 0: { start: { line: 1, column: 0 }, end: { line: 1, column: 12 } } },
    fnMap: {},
    branchMap: {},
    s: { 0: 1, 1: null },
    f: {},
    b: {},
  });

  // Control: the raw orphan crashes istanbul-lib-coverage's merge (the issue).
  assert.throws(
    () => {
      const m = libCoverage.createCoverageMap({});
      m.addFileCoverage(orphanFile());
      m.addFileCoverage(orphanFile());
    },
    /Cannot destructure property 'start'/,
    'control: a raw orphan must crash istanbul-lib-coverage merge',
  );

  // Routing the same coverage through remapCoverageMap reconciles the invariant.
  // The entry has no inputSourceMap, so it takes the passthrough branch, which
  // still drops the orphan.
  const cleaned = JSON.parse(remapCoverageMap(JSON.stringify({ '/x/mod.ts': orphanFile() })));
  const fc = cleaned['/x/mod.ts'];
  const orphans = Object.keys(fc.s).filter((k) => !(k in fc.statementMap));
  assert.deepEqual(orphans, [], `remapCoverageMap must drop orphan s keys, found ${orphans}`);
  assert.deepEqual(Object.keys(fc.s), ['0'], 'only the mapped statement counter survives');

  // The cleaned coverage now merges without throwing.
  assert.doesNotThrow(() => {
    const m = libCoverage.createCoverageMap({});
    m.addFileCoverage(cleaned['/x/mod.ts']);
    m.addFileCoverage(JSON.parse(remapCoverageMap(JSON.stringify({ '/x/mod.ts': orphanFile() })))['/x/mod.ts']);
  }, 'reconciled coverage must merge cleanly in istanbul-lib-coverage (issue #107)');

  console.log('  [PASS] remapCoverageMap drops orphan counters so nyc merge succeeds (issue #107)');
}

// Test: composeInputSourceMap-emitted code emits NO dangling statement or
// function counter when a line is unmapped (issue #107 producer guard). Scan
// `s` and `f` for counters emitted against ids pruned from the map; a
// `++cov.s[id]` against a missing slot produces a null orphan.
{
  const generated = [
    'const a = 1;',
    'const b = 2;',
    'function used(x) { return x + 1; }', // line 3: maps
    'function dropped(y) { return y * 2; }', // line 4: UNMAPPED, must not be instrumented
    'const c = used(a) + dropped(b);', // line 5: maps
    'globalThis.__ran = a + b + c;',
    '',
  ].join('\n');
  // Lines 1,2,3 and 5,6 map; line 4 is unmapped (empty mapping segment).
  const inputSourceMap = JSON.stringify({
    version: 3,
    sources: ['original.ts'],
    names: [],
    mappings: 'AAAA;AACA;AACA;;AAEA;AACA',
    sourcesContent: ['x'.repeat(200)],
  });

  const r = instrument(generated, 'mod.ts', {
    coverageVariable: '__cov107s',
    inputSourceMap,
    composeInputSourceMap: true,
  });
  const map = JSON.parse(r.coverageMap);

  const sRefs = [...new Set([...r.code.matchAll(/\.s\[(\d+)\]/g)].map((m) => m[1]))];
  const fRefs = [...new Set([...r.code.matchAll(/\.f\[(\d+)\]/g)].map((m) => m[1]))];
  const sDangling = sRefs.filter((id) => !(id in map.statementMap));
  const fDangling = fRefs.filter((id) => !(id in map.fnMap));
  assert.deepEqual(sDangling, [], `no dangling statement counter in emitted code, got ${sDangling}`);
  assert.deepEqual(fDangling, [], `no dangling function counter in emitted code, got ${fDangling}`);

  // Execute: no orphan s/f keys appear at runtime (the null-orphan failure mode).
  const g = {};
  assert.doesNotThrow(() => {
    new Function('globalThis', r.code)(g);
  }, 'instrumented eager-composed code must run without TypeError (issue #107)');
  const fc = g.__cov107s['original.ts'];
  const sOrphans = Object.keys(fc.s).filter((k) => !(k in fc.statementMap));
  const fOrphans = Object.keys(fc.f).filter((k) => !(k in fc.fnMap));
  assert.deepEqual(sOrphans, [], `runtime produced orphan s keys: ${sOrphans}`);
  assert.deepEqual(fOrphans, [], `runtime produced orphan f keys: ${fOrphans}`);
  for (const [id, count] of Object.entries(fc.s)) {
    assert.ok(Number.isFinite(count), `statement counter ${id} must be finite (no NaN/null), got ${count}`);
  }

  console.log('  [PASS] composeInputSourceMap emits no dangling statement/function counters (issue #107)');
}

// Test: the verbatim real-world FileCoverage from issue #107 (statementMap
// jumps 2 -> 4, s["3"] is null) is reconciled by remapCoverageMap so the
// downstream istanbul-lib-coverage merge that nyc runs no longer crashes. This
// is the reporter's exact shape (names mocked) captured from window.__coverage__.
{
  const raw = JSON.parse(
    readFileSync(join(__dirname, '__fixtures__', 'issue-107-orphan-statement.json'), 'utf8'),
  );

  // Confirm the fixture really carries the orphan the issue describes.
  assert.ok(!('3' in raw.statementMap), 'fixture precondition: statementMap has no key "3"');
  assert.equal(raw.s['3'], null, 'fixture precondition: s["3"] is the null orphan');

  // Control: the raw shape crashes istanbul-lib-coverage merge (the reporter's stack).
  assert.throws(
    () => {
      const m = libCoverage.createCoverageMap({});
      m.addFileCoverage(structuredClone(raw));
      m.addFileCoverage(structuredClone(raw));
    },
    /Cannot destructure property 'start'/,
    'control: the raw issue-107 coverage must crash istanbul-lib-coverage merge',
  );

  // The fixture is already composed (no inputSourceMap), so it takes the
  // passthrough branch of remapCoverageMap, which still reconciles the orphan.
  const cleaned = JSON.parse(remapCoverageMap(JSON.stringify({ [raw.path]: raw })));
  const fc = cleaned[raw.path];
  const orphans = Object.keys(fc.s).filter((k) => !(k in fc.statementMap));
  assert.deepEqual(orphans, [], `reconciled coverage must have no orphan s keys, found ${orphans}`);
  // Every other surviving statement counter is preserved (35 of 36 keys; only "3" drops).
  assert.equal(Object.keys(fc.s).length, 35, 'only the single orphan statement counter is dropped');
  assert.ok('0' in fc.s && '35' in fc.s, 'legitimate statement counters are preserved');

  // The cleaned coverage now merges without throwing.
  assert.doesNotThrow(() => {
    const m = libCoverage.createCoverageMap({});
    m.addFileCoverage(structuredClone(fc));
    m.addFileCoverage(structuredClone(fc));
  }, 'reconciled issue-107 coverage must merge cleanly in istanbul-lib-coverage');

  console.log('  [PASS] verbatim issue #107 FileCoverage is reconciled and merges (regression fixture)');
}

// Test: trackOptionalChainBranches gates optional-chain (?.) instrumentation (issue #108)
{
  const source = 'function f(e, cb) { return e?.stderr?.replace(/x/, "y") ?? cb?.(1); }';

  // Default (flag omitted): each ?. link is a branch and the _oc helper is emitted.
  const tracked = instrument(source, 'oc.js', { coverageVariable: '__ocOn' });
  const trackedMap = JSON.parse(tracked.coverageMap);
  const trackedOc = Object.values(trackedMap.branchMap).filter((e) => e.type === 'optional-chain');
  assert.equal(trackedOc.length, 3, 'default tracks all three ?. links (two member, one call)');
  assert.ok(tracked.code.includes('_oc('), 'default emits the _oc helper');

  // trackOptionalChainBranches: false leaves the chain native.
  const native = instrument(source, 'oc.js', {
    coverageVariable: '__ocOff',
    trackOptionalChainBranches: false,
  });
  const nativeMap = JSON.parse(native.coverageMap);
  const nativeOc = Object.values(nativeMap.branchMap).filter((e) => e.type === 'optional-chain');
  assert.equal(nativeOc.length, 0, 'no optional-chain branches when tracking is off');
  assert.ok(!native.code.includes('_oc('), 'the _oc helper must not be emitted when off');
  // The ?? is still a logical branch; only ?. tracking is suppressed.
  assert.ok(
    Object.values(nativeMap.branchMap).some((e) => e.type === 'binary-expr'),
    'non-optional-chain branches are still tracked',
  );
  // Statement coverage is unaffected by the toggle.
  assert.equal(
    Object.keys(nativeMap.statementMap).length,
    Object.keys(trackedMap.statementMap).length,
    'statement coverage is identical with and without optional-chain tracking',
  );

  // The native-mode code still runs and preserves ?. semantics.
  const g = {};
  assert.doesNotThrow(() => {
    new Function('globalThis', `${native.code}\nglobalThis.__r = eval('f')(null, null);`)(g);
  }, 'native optional-chain code runs without throwing');
  assert.equal(g.__r, undefined, 'e?.stderr on null short-circuits to undefined, ?? cb?.(1) on null cb stays undefined');

  console.log('  [PASS] trackOptionalChainBranches gates optional-chain instrumentation (issue #108)');
}

// Test: createOxcInstrumenter forwards trackOptionalChainBranches and validates it
{
  const source = 'export function f(e) { return e?.a?.b; }';

  const on = createOxcInstrumenter();
  on.instrumentSync(source, 'oc.ts');
  const onOc = Object.values(on.lastFileCoverage().branchMap).filter(
    (e) => e.type === 'optional-chain',
  );
  assert.equal(onOc.length, 2, 'adapter defaults to tracking ?. links');

  const off = createOxcInstrumenter({ trackOptionalChainBranches: false });
  const offCode = off.instrumentSync(source, 'oc.ts');
  const offOc = Object.values(off.lastFileCoverage().branchMap).filter(
    (e) => e.type === 'optional-chain',
  );
  assert.equal(offOc.length, 0, 'adapter honors trackOptionalChainBranches: false');
  assert.ok(!offCode.includes('_oc('), 'adapter emits no _oc helper when off');

  assert.throws(
    () => createOxcInstrumenter({ trackOptionalChainBranches: 'no' }),
    /trackOptionalChainBranches.*must be a boolean/,
    'non-boolean trackOptionalChainBranches throws TypeError',
  );

  console.log('  [PASS] createOxcInstrumenter forwards/validates trackOptionalChainBranches');
}

// Test: tracked optional member calls preserve their receiver (issue #192)
{
  const source = `function flatten(oldData) {
  return oldData?.pages?.flat?.() || [];
}
const holder = {
  value: 7,
  method(add) { return this.value + add; },
};
const methodName = 'method';
class Base {
  constructor() { this.value = 11; }
  method() { return this.value; }
}
class Derived extends Base {
  run() { return super.method?.(); }
}
globalThis.__result = {
  flat: flatten({ pages: [[1, 2], [3, 4]] }),
  computed: holder[methodName]?.(5),
  parenthesized: (holder.method)?.(6),
  superCall: new Derived().run(),
};`;
  const result = instrument(source, 'optional-member-call.js');
  const optionalBranches = Object.values(JSON.parse(result.coverageMap).branchMap).filter(
    (entry) => entry.type === 'optional-chain',
  );
  const sharedGlobal = {};

  assert.equal(
    optionalBranches.length,
    2,
    'member links stay tracked while receiver-bound calls stay native',
  );
  assert.doesNotThrow(() => {
    new Function('globalThis', result.code)(sharedGlobal);
  }, 'instrumenting a tracked optional member call must preserve its receiver');
  assert.deepEqual(sharedGlobal.__result, {
    flat: [1, 2, 3, 4],
    computed: 12,
    parenthesized: 13,
    superCall: 11,
  });

  console.log('  [PASS] tracked optional member calls preserve their receiver (issue #192)');
}

// Test: nameCallbackArguments names callback arguments from the callee
{
  const source = 'arr.map((x) => x + 1);\nnew Promise((resolve) => resolve(1));';

  const off = instrument(source, 'cb.js');
  const offNames = Object.values(JSON.parse(off.coverageMap).fnMap).map((f) => f.name);
  assert.ok(
    offNames.every((n) => n.startsWith('(anonymous_')),
    `default leaves callback args anonymous, got ${JSON.stringify(offNames)}`,
  );

  const on = instrument(source, 'cb.js', { nameCallbackArguments: true });
  const onNames = Object.values(JSON.parse(on.coverageMap).fnMap).map((f) => f.name);
  assert.deepEqual(onNames, ['map', 'Promise'], 'callee names surface with the option on');

  console.log('  [PASS] nameCallbackArguments names callback arguments from the callee');
}

// Test: createOxcInstrumenter forwards nameCallbackArguments and validates it
{
  const source = 'export const f = (arr) => arr.filter((x) => x > 0);';

  const off = createOxcInstrumenter();
  off.instrumentSync(source, 'cb.ts');
  const offNames = Object.values(off.lastFileCoverage().fnMap).map((f) => f.name);
  assert.ok(
    offNames.includes('f') && offNames.some((n) => n.startsWith('(anonymous_')),
    `adapter defaults to leaving callback args anonymous, got ${JSON.stringify(offNames)}`,
  );

  const on = createOxcInstrumenter({ nameCallbackArguments: true });
  on.instrumentSync(source, 'cb.ts');
  const onNames = Object.values(on.lastFileCoverage().fnMap).map((f) => f.name);
  assert.ok(
    onNames.includes('f') && onNames.includes('filter'),
    `adapter honors nameCallbackArguments: true, got ${JSON.stringify(onNames)}`,
  );

  assert.throws(
    () => createOxcInstrumenter({ nameCallbackArguments: 'yes' }),
    /nameCallbackArguments.*must be a boolean/,
    'non-boolean nameCallbackArguments throws TypeError',
  );

  console.log('  [PASS] createOxcInstrumenter forwards/validates nameCallbackArguments');
}

// Test: ifConsequentLocation records the consequent for arm 0 of an `if`
{
  const source = 'if (a) { b(); } else { c(); }';
  const span = (loc) => [loc.start.line, loc.start.column, loc.end.line, loc.end.column];
  const ifBranch = (coverage) =>
    Object.values(coverage.branchMap).find((branch) => branch.type === 'if');

  const off = ifBranch(JSON.parse(instrument(source, 'if.js').coverageMap));
  assert.deepEqual(span(off.locations[0]), span(off.loc), 'default arm 0 is the whole if');

  const on = ifBranch(
    JSON.parse(instrument(source, 'if.js', { ifConsequentLocation: true }).coverageMap),
  );
  assert.deepEqual(span(on.loc), [1, 0, 1, 29], 'branch loc stays the whole if');
  assert.deepEqual(span(on.locations[0]), [1, 7, 1, 15], 'arm 0 is the consequent');
  assert.deepEqual(span(on.locations[1]), [1, 21, 1, 29], 'arm 1 is the alternate');

  const strict = ifBranch(
    JSON.parse(
      instrument(source, 'if.js', { ifConsequentLocation: true, compat: 'istanbul' })
        .coverageMap,
    ),
  );
  assert.deepEqual(span(strict.locations[0]), span(strict.loc), 'istanbul compat wins');

  console.log('  [PASS] ifConsequentLocation records the consequent for arm 0');
}

// Test: createOxcInstrumenter forwards ifConsequentLocation and validates it
{
  const source = 'export const f = (a) => {\n  if (a) {\n    return 1;\n  }\n  return 2;\n};\n';
  const ifBranch = (instrumenter) =>
    Object.values(instrumenter.lastFileCoverage().branchMap).find(
      (branch) => branch.type === 'if',
    );

  const off = createOxcInstrumenter();
  off.instrumentSync(source, 'if.ts');
  assert.equal(ifBranch(off).locations[0].start.column, 2, 'adapter default is the whole if');

  const on = createOxcInstrumenter({ ifConsequentLocation: true });
  on.instrumentSync(source, 'if.ts');
  const arm = ifBranch(on).locations[0];
  assert.deepEqual(
    [arm.start.line, arm.start.column, arm.end.line, arm.end.column],
    [2, 9, 4, 3],
    'adapter honors ifConsequentLocation: true',
  );

  assert.throws(
    () => createOxcInstrumenter({ ifConsequentLocation: 'yes' }),
    /ifConsequentLocation.*must be a boolean/,
    'non-boolean ifConsequentLocation throws TypeError',
  );

  console.log('  [PASS] createOxcInstrumenter forwards/validates ifConsequentLocation');
}

// Test: istanbul-lib-source-maps getMapping byte-parity (issue #111)
//
// The remap path now resolves surviving positions through an istanbul
// `getMapping`-equivalent range remap (start = greatest-lower-bound, end = next
// original segment or end-of-line) instead of a direct per-position lookup. This
// harness instruments a fixture, embeds a known input source map, and remaps it
// through BOTH `istanbul-lib-source-maps@5`'s `createSourceMapStore().transformCoverage`
// AND our `remapCoverageMap(..., { dropUnmapped: true })`, then asserts the
// surviving spans are byte-for-byte equal. It is the only check that catches the
// exact-match-vs-least-upper-bound gap in the next-segment reverse lookup.
{
  // istanbul-lib-coverage / -source-maps are CommonJS: named exports surface on
  // the dynamic-import `default` namespace, not the top level.
  const { createSourceMapStore } = (await import('istanbul-lib-source-maps')).default;
  const { GenMapping, addMapping, setSourceContent, toEncodedMap } = await import(
    '@jridgewell/gen-mapping'
  );
  // @babel/core 7 is CJS (transformSync lives on the dynamic-import `.default`
  // namespace); @babel/core 8 is ESM with no default export and named exports
  // on the top-level namespace. Support both by preferring `.default`.
  const babelMod = await import('@babel/core');
  const babel = babelMod.default ?? babelMod;

  const utf16LineLength = (content, line1) => {
    const raw = content.split('\n')[line1 - 1] ?? '';
    const line = raw.endsWith('\r') ? raw.slice(0, -1) : raw;
    return line.length; // JS string length is the UTF-16 code-unit count
  };

  // istanbul stores the end-of-line sentinel as `Infinity`; our `u32` clamps it
  // to the original line's UTF-16 length. Normalize istanbul to the same value
  // so the comparison is apples-to-apples. Finite columns pass through.
  const normEndColumn = (col, line, content) =>
    col === Infinity || col === null || col === undefined ? utf16LineLength(content, line) : col;

  const spanKey = (loc, content) => {
    const sl = loc.start.line ?? 0;
    const sc = loc.start.column ?? 0;
    const el = loc.end.line ?? 0;
    const ec = normEndColumn(loc.end.column, el, content);
    return `${sl}:${sc}:${el}:${ec}`;
  };

  const statementSet = (data, content) =>
    new Set(Object.values(data.statementMap).map((loc) => spanKey(loc, content)));
  const functionSet = (data, content) =>
    new Set(
      Object.values(data.fnMap).map(
        (fn) => `${fn.name}|${spanKey(fn.decl, content)}|${spanKey(fn.loc, content)}`,
      ),
    );
  const branchSet = (data, content) =>
    new Set(
      Object.values(data.branchMap).map(
        (b) =>
          `${b.type}|${spanKey(b.loc, content)}|${b.locations
            .map((l) => spanKey(l, content))
            .join(',')}`,
      ),
    );

  const sortedArray = (set) => [...set].sort();

  // Core: instrument the intermediate JS with `inputMap` embedded, remap through
  // istanbul and through us, and assert the surviving spans match column-by-column.
  // Single-source maps only (cross-source arm/decl source-equality is istanbul
  // drop-semantics territory, out of #111 scope).
  const compareRemap = async (label, intermediateJs, inputMap, sourceContent) => {
    const result = instrument(intermediateJs, 'intermediate.js', {
      inputSourceMap: JSON.stringify(inputMap),
    });
    const coverageData = JSON.parse(result.coverageMap);

    // istanbul path: createSourceMapStore().transformCoverage reads the embedded
    // inputSourceMap and drops unmappable entries (transformer.js semantics).
    const istanbulMap = createSourceMapStore().transformCoverage(
      createCoverageMap({ 'intermediate.js': coverageData }),
    );
    const transformed = await istanbulMap;

    // our path: drop_unmapped mirrors istanbul's drop semantics.
    const ours = JSON.parse(
      remapCoverageMap(JSON.stringify({ 'intermediate.js': coverageData }), {
        dropUnmapped: true,
      }),
    );

    // istanbul keys the surviving file by `pathutils.relativeTo` (which can
    // absolutize the source), while we keep the source path verbatim. The keys
    // differ by path-resolution policy, not span correctness, so compare the
    // span sets pairwise. These fixtures are single-source, so there is exactly
    // one surviving file on each side.
    const istanbulFiles = transformed.files().sort();
    const ourFiles = Object.keys(ours).sort();
    assert.equal(
      ourFiles.length,
      istanbulFiles.length,
      `${label}: same surviving file count (ours=${ourFiles}, istanbul=${istanbulFiles})`,
    );

    for (let i = 0; i < istanbulFiles.length; i++) {
      const istanbulCov = transformed.fileCoverageFor(istanbulFiles[i]).data;
      const ourCov = ours[ourFiles[i]];
      assert.deepEqual(
        sortedArray(statementSet(ourCov, sourceContent)),
        sortedArray(statementSet(istanbulCov, sourceContent)),
        `${label}: statement spans match istanbul`,
      );
      assert.deepEqual(
        sortedArray(functionSet(ourCov, sourceContent)),
        sortedArray(functionSet(istanbulCov, sourceContent)),
        `${label}: function spans match istanbul`,
      );
      assert.deepEqual(
        sortedArray(branchSet(ourCov, sourceContent)),
        sortedArray(branchSet(istanbulCov, sourceContent)),
        `${label}: branch spans match istanbul`,
      );
    }
    return ours;
  };

  // Build an input source map from explicit token mappings (via gen-mapping),
  // then run it through the shared remap+compare core.
  const parityCheck = async (label, sourcePath, sourceContent, intermediateJs, mappings) => {
    const gm = new GenMapping({ file: 'intermediate.js' });
    setSourceContent(gm, sourcePath, sourceContent);
    for (const m of mappings) {
      addMapping(gm, { generated: m.gen, source: sourcePath, original: m.orig });
    }
    return compareRemap(label, intermediateJs, toEncodedMap(gm), sourceContent);
  };

  // Reporter case 1: an exclusive end that falls past the last segment. Direct
  // lookup truncated `state.someVeryLongPropertyName1` back to `state.`; getMapping
  // widens it to the end of the original line.
  {
    const content = 'state.someVeryLongPropertyName1;\n';
    const ours = await parityCheck(
      'reporter case 1 (end widening)',
      'src/app.ts',
      content,
      'state.someVeryLongPropertyName1;\n',
      [
        { gen: { line: 1, column: 0 }, orig: { line: 1, column: 0 } }, // `state`
        { gen: { line: 1, column: 6 }, orig: { line: 1, column: 6 } }, // property start
      ],
    );
    const stmt = Object.values(ours['src/app.ts'].statementMap).find((l) => l.start.column === 0);
    assert.equal(
      stmt.end.column,
      'state.someVeryLongPropertyName1;'.length,
      'statement end widens to the end of the original line, not back to `state.` (col 6)',
    );
  }

  // Reporter case 1b: end widens to the NEXT original segment (a Mapped end, not
  // end-of-line) -- the case the exact-match reverse lookup gets wrong.
  {
    const ours = await parityCheck(
      'reporter case 1b (widen to next segment)',
      'src/app.ts',
      'a.b; c.d;\n',
      'a.b; c.d;\n',
      [
        { gen: { line: 1, column: 0 }, orig: { line: 1, column: 0 } }, // a
        { gen: { line: 1, column: 2 }, orig: { line: 1, column: 2 } }, // b
        { gen: { line: 1, column: 5 }, orig: { line: 1, column: 10 } }, // c
        { gen: { line: 1, column: 7 }, orig: { line: 1, column: 12 } }, // d
      ],
    );
    const stmt = Object.values(ours['src/app.ts'].statementMap).find(
      (l) => l.start.line === 1 && l.start.column === 0,
    );
    assert.equal(
      stmt.end.column,
      10,
      'first statement end widens to the next original segment (col 10), not the truncated col 2',
    );
  }

  // Reporter case 2: a 1-char arrow declaration that balloons to its enclosing
  // span. istanbul-lib-instrument and oxc both record a 1-char decl; the remap is
  // what widens it.
  {
    await parityCheck(
      'reporter case 2 (arrow decl balloon)',
      'src/app.ts',
      'const make = defineAsyncComponent(() => import("./X.vue"));\n',
      'const make = defineAsyncComponent(() => import("./X.vue"));\n',
      [
        { gen: { line: 1, column: 0 }, orig: { line: 1, column: 0 } }, // const
        { gen: { line: 1, column: 6 }, orig: { line: 1, column: 6 } }, // make
        { gen: { line: 1, column: 13 }, orig: { line: 1, column: 13 } }, // defineAsyncComponent
        { gen: { line: 1, column: 34 }, orig: { line: 1, column: 34 } }, // () => arrow
        { gen: { line: 1, column: 40 }, orig: { line: 1, column: 40 } }, // import(...)
      ],
    );
  }

  // Multi-line fixture exercising functions, statements, and a ternary branch
  // across several mapped lines.
  {
    const content = [
      'export function classify(value) {', // line 1
      '  const label = value > 0 ? "pos" : "neg";', // line 2
      '  return label;', // line 3
      '}', // line 4
      '',
    ].join('\n');
    await parityCheck('multi-line functions + branch', 'src/classify.ts', content, content, [
      { gen: { line: 1, column: 0 }, orig: { line: 1, column: 0 } },
      { gen: { line: 1, column: 16 }, orig: { line: 1, column: 16 } },
      { gen: { line: 1, column: 25 }, orig: { line: 1, column: 25 } },
      { gen: { line: 2, column: 2 }, orig: { line: 2, column: 2 } },
      { gen: { line: 2, column: 17 }, orig: { line: 2, column: 17 } },
      { gen: { line: 2, column: 25 }, orig: { line: 2, column: 25 } },
      { gen: { line: 2, column: 36 }, orig: { line: 2, column: 36 } },
      { gen: { line: 3, column: 2 }, orig: { line: 3, column: 2 } },
      { gen: { line: 3, column: 9 }, orig: { line: 3, column: 9 } },
      { gen: { line: 4, column: 0 }, orig: { line: 4, column: 0 } },
    ]);
  }

  // Real-transpiler case: a REAL @babel/core transform emits the input source
  // map (real VLQ, segments at token starts, sourcesContent), exercising the
  // sparse-segment scenario that hand-built maps don't. An inline rename plugin
  // shortens long identifiers, which collapses member chains and shifts columns
  // hard, so exclusive ends land between segments (the exact #111 case).
  {
    const realSrc = [
      "import { defineAsyncComponent } from 'vue';",
      'const state = { someVeryLongPropertyName1: 0, anotherQuiteLongFieldName: 1 };',
      'function classify(inputValue) {',
      "  const label = inputValue > 0 ? 'positive' : 'negative';",
      '  return state.someVeryLongPropertyName1 + label.length;',
      '}',
      "const lazyComponentFactory = defineAsyncComponent(() => importHelper('./Widget.vue'));",
      'const mapped = [1, 2, 3].map((eachNumber) => eachNumber * state.anotherQuiteLongFieldName);',
      'export { classify, lazyComponentFactory, mapped };',
      '',
    ].join('\n');

    // Pass 1: collect a rename table for identifiers longer than 4 chars (skip
    // import/export specifiers so the module bindings stay resolvable).
    const renames = new Map();
    let renameCounter = 0;
    const collectRenames = () => ({
      visitor: {
        Identifier(path) {
          const name = path.node.name;
          if (name.length <= 4) return;
          if (path.parentPath.isImportSpecifier() || path.parentPath.isExportSpecifier()) return;
          if (!renames.has(name)) renames.set(name, `v${renameCounter++}`);
        },
      },
    });
    babel.transformSync(realSrc, {
      filename: 'src/app.js',
      plugins: [collectRenames],
      sourceType: 'module',
    });
    const applyRenames = () => ({
      visitor: {
        Identifier(path) {
          const renamed = renames.get(path.node.name);
          if (!renamed) return;
          if (path.parentPath.isImportSpecifier() || path.parentPath.isExportSpecifier()) return;
          path.node.name = renamed;
        },
      },
    });
    const out = babel.transformSync(realSrc, {
      filename: 'src/app.js',
      plugins: [applyRenames],
      sourceType: 'module',
      sourceMaps: true,
      compact: false,
    });
    const inputMap = out.map;
    if (!inputMap.sourcesContent) inputMap.sourcesContent = [realSrc];

    const ours = await compareRemap(
      'real @babel/core-emitted map',
      out.code,
      inputMap,
      realSrc,
    );

    // Prove the fix is load-bearing on a real map: at least one surviving
    // single-line statement must span more than one column (ends are widened to
    // the full token, not truncated back to a previous segment).
    const file = Object.keys(ours)[0];
    const widened = Object.values(ours[file].statementMap).filter(
      (l) => l.start.line === l.end.line && l.end.column - l.start.column > 1,
    );
    assert.ok(
      widened.length > 0,
      'real-map remap produces widened multi-column statement spans (ends not truncated)',
    );
  }

  console.log('  [PASS] istanbul-lib-source-maps getMapping byte-parity (issue #111)');
}

// Test: inline `export const fn = () => {}` counts its declaration statement
// at module evaluation (issue #114). Requires real module evaluation because
// `export` is module-only; `new Function` cannot host it. The initializer runs
// when the module is loaded, so s['0'] must be 1 even though `fn` is never
// called. The non-exported form is the control that already worked.
{
  const initStatement = async (src, cv) => {
    const file = join(tmpdir(), `oxc-cov-${cv}-${process.pid}.mjs`);
    writeFileSync(file, instrument(src, 'm.ts', { coverageVariable: cv }).code);
    try {
      await import(pathToFileURL(file).href); // module evaluation only, fn never called
      return Object.values(globalThis[cv])[0].s['0'];
    } finally {
      rmSync(file, { force: true });
    }
  };

  const exported = await initStatement('export const fn = () => { return 1; };\n', '__oxc114_a');
  const plain = await initStatement('const fn = () => { return 1; };\nexport { fn };\n', '__oxc114_b');
  assert.equal(exported, 1, 'export const arrow declaration statement must count at module eval');
  assert.equal(plain, 1, 'non-exported const arrow declaration statement must count (control)');
  console.log('  [PASS] export const arrow declaration statement counts at module eval (issue #114)');
}

// Test: strictNullChecks controls how a nullable union lands in design:type.
// Wrong value here is silent: the code still runs, but NestJS DI, TypeORM
// column inference, and class-validator read that metadata and would see a
// different constructor than tsc emitted.
{
  const source =
    'function D() { return function (_t, _k) {}; }\n' +
    'export class C { @D() foo: string | null = null; }\n';
  const opts = {
    stripTypescript: true,
    experimentalDecorators: true,
    emitDecoratorMetadata: true,
  };

  const strict = instrument(source, 'c.ts', { ...opts, strictNullChecks: true });
  assert.ok(
    strict.code.includes('_decorateMetadata("design:type", Object)'),
    'strictNullChecks: true must widen `string | null` to Object',
  );

  const loose = instrument(source, 'c.ts', { ...opts, strictNullChecks: false });
  assert.ok(
    loose.code.includes('_decorateMetadata("design:type", String)'),
    'strictNullChecks: false must elide null, leaving String',
  );

  const omitted = instrument(source, 'c.ts', opts);
  assert.equal(
    omitted.code.toString(),
    strict.code.toString(),
    'omitting strictNullChecks must default to true (tsc under `strict`)',
  );

  assert.throws(
    () => createOxcInstrumenter({ strictNullChecks: 'yes' }),
    /strictNullChecks.*must be a boolean or undefined/,
    'createOxcInstrumenter must reject a non-boolean strictNullChecks',
  );

  console.log('  [PASS] strictNullChecks controls nullable design:type metadata');
}

// Test: reporters and summary over real instrumented coverage
{
  const root = mkdtempSync(join(tmpdir(), 'oxc-cov-report-'));
  mkdirSync(join(root, 'src'));
  const sources = {
    'src/math.js':
      'function add(a, b) { return a + b; }\nfunction sub(a, b) {\n  if (a > b) { return a - b; }\n  return b - a;\n}\nadd(1, 2);\nsub(3, 1);\n',
    'src/unused.js': 'function never() {\n  return 1;\n}\n',
  };
  const coverage = {};
  for (const [relative, source] of Object.entries(sources)) {
    const filename = join(root, relative);
    writeFileSync(filename, source);
    const result = instrument(source, filename);
    const sharedGlobal = {};
    new Function('globalThis', result.code)(sharedGlobal);
    coverage[filename] = sharedGlobal.__coverage__[filename];
  }
  const coverageJson = JSON.stringify(coverage);

  // The summary must agree with istanbul-lib-coverage, metric for metric.
  const summary = summarizeCoverage(coverageJson);
  const istanbulMap = createCoverageMap(JSON.parse(coverageJson));
  const metrics = ['lines', 'statements', 'functions', 'branches'];
  const pick = (s) =>
    Object.fromEntries(metrics.map((m) => [m, { total: s[m].total, covered: s[m].covered, skipped: s[m].skipped, pct: s[m].pct }]));
  assert.deepEqual(pick(summary.total), pick(istanbulMap.getCoverageSummary().toJSON()));
  assert.deepEqual(Object.keys(summary.files).sort(), Object.keys(coverage).sort());
  for (const filename of Object.keys(coverage)) {
    assert.deepEqual(
      pick(summary.files[filename]),
      pick(istanbulMap.fileCoverageFor(filename).toSummary().toJSON()),
      `summary for ${filename} must match istanbul-lib-coverage`,
    );
  }
  assert.equal(summary.files[join(root, 'src/unused.js')].functions.covered, 0);
  console.log('  [PASS] summarizeCoverage matches istanbul-lib-coverage');

  // json-summary carries the same numbers as summarizeCoverage.
  const jsonSummary = JSON.parse(createReport(coverageJson, 'json-summary'));
  assert.deepEqual(pick(jsonSummary.total), pick(summary.total));
  console.log('  [PASS] createReport json-summary');

  // lcov paths are relative to rootDir, and absolute without it.
  const lcov = createReport(coverageJson, 'lcov', { rootDir: root });
  assert.match(lcov, /^SF:src\/math\.js$/m);
  assert.match(lcov, /^FNF:2$/m);
  assert.equal((lcov.match(/^end_of_record$/gm) ?? []).length, 2);
  const lcovAbsolute = createReport(coverageJson, 'lcov');
  assert(lcovAbsolute.includes(`SF:${join(root, 'src/math.js')}`), 'without rootDir, paths stay verbatim');
  console.log('  [PASS] createReport lcov honours rootDir');

  const cobertura = createReport(coverageJson, 'cobertura', { rootDir: root });
  assert(cobertura.startsWith('<?xml'), 'cobertura is an XML document');
  assert(cobertura.includes('filename="src/math.js"'), 'cobertura filename is relative to rootDir');
  console.log('  [PASS] createReport cobertura');

  const text = createReport(coverageJson, 'text');
  assert.match(text, /math\.js/);
  assert.match(text, /unused\.js/);
  const textSummary = createReport(coverageJson, 'text-summary');
  assert.match(textSummary, /Statements\s*:/);
  assert.match(textSummary, /Branches\s*:/);
  console.log('  [PASS] createReport text and text-summary');

  assert.throws(() => createReport(coverageJson, 'html'), /writeHtmlReport/);
  assert.throws(() => createReport(coverageJson, 'clover'), /clover/);
  assert.throws(() => createReport('{not json', 'lcov'), /coverage/i);
  assert.throws(() => summarizeCoverage('[]'), /coverage/i);
  console.log('  [PASS] createReport rejects html, unknown formats, and invalid input');

  // html writes a directory tree and is native-only.
  const isWasi = !!process.env.NAPI_RS_FORCE_WASI;
  const outDir = join(root, 'html-report');
  if (isWasi) {
    assert.throws(() => writeHtmlReport(coverageJson, outDir, { rootDir: root }), /native/);
    console.log('  [PASS] writeHtmlReport reports that it needs the native binding under WASI');
  } else {
    writeHtmlReport(coverageJson, outDir, { rootDir: root, greenThreshold: 90 });
    assert(existsSync(join(outDir, 'index.html')), 'html report writes index.html');
    // The page tree starts at the common path prefix (here `src/`), as in Istanbul.
    assert(existsSync(join(outDir, 'math.js.html')), 'html report writes one page per file');
    assert(existsSync(join(outDir, 'unused.js.html')), 'html report writes one page per file');
    assert.throws(
      () => writeHtmlReport(coverageJson, outDir, { greenThreshold: 101 }),
      /threshold/,
      'an out-of-range greenThreshold is rejected',
    );
    console.log('  [PASS] writeHtmlReport writes the html tree');
  }
  rmSync(root, { recursive: true, force: true });
}

// Test: karma-coverage adapter (oxc-coverage-instrument/karma)
{
  // karma-coverage reads `require(...).Instrumenter` from each entry in
  // `coverageReporter.instrumenters`, so the subpath must export it.
  const require = createRequire(import.meta.url);
  const karmaModule = require('oxc-coverage-instrument/karma');
  assert.equal(typeof karmaModule.Instrumenter, 'function', 'karma subpath exports Instrumenter');
  assert.equal(karmaModule.Instrumenter, KarmaInstrumenter, 'subpath and file export the same class');
  assert(createKarmaInstrumenter() instanceof KarmaInstrumenter, 'factory returns an Instrumenter');

  const source = [
    'function pick(a, b) {',
    '  if (a) { return a; }',
    '  const fn = () => b || 0;',
    '  return fn();',
    '}',
    'pick(0, 2);',
  ].join('\n');

  // karma-coverage always adds `autoWrap` and `codeGenerationOptions`.
  const karmaOptions = { autoWrap: true, codeGenerationOptions: null, compat: 'istanbul' };
  const oxc = new KarmaInstrumenter(karmaOptions);
  let calls = 0;
  let instrumented;
  oxc.instrument(source, '/project/src/pick.js', (error, code) => {
    calls += 1;
    assert.equal(error, null, 'instrument() reports no error for valid code');
    instrumented = code;
  });
  assert.equal(calls, 1, 'instrument() calls the callback one time');
  assert.equal(typeof instrumented, 'string', 'instrument() gives a string to the callback');
  assert.equal(oxc.lastSourceMap(), null, 'lastSourceMap() is null without produceSourceMap');

  const istanbul = createInstrumenter({ autoWrap: true, codeGenerationOptions: null });
  istanbul.instrumentSync(source, '/project/src/pick.js');
  assert.deepEqual(
    oxc.lastFileCoverage(),
    // karma sends coverage through JSON, which drops istanbul's undefined
    // positions in an implicit else location.
    JSON.parse(JSON.stringify(istanbul.lastFileCoverage())),
    'compat istanbul coverage equals istanbul-lib-instrument for karma options',
  );

  const sandbox = {};
  new Function('globalThis', instrumented)(sandbox);
  const counts = sandbox.__coverage__['/project/src/pick.js'];
  assert.deepEqual(counts.f, { 0: 1, 1: 1 }, 'instrumented code counts both functions');
  console.log('  [PASS] karma adapter matches istanbul-lib-instrument through instrument()');
}

{
  const oxc = new KarmaInstrumenter({ coverageVariable: '__karma_cov__', produceSourceMap: true });
  const code = oxc.instrumentSync('var x = 1;', 'x.js');
  assert(code.includes('__karma_cov__'), 'coverageVariable is forwarded');
  const map = oxc.lastSourceMap();
  assert.equal(map?.version, 3, 'lastSourceMap() returns a v3 map with produceSourceMap');
  assert.deepEqual(map.sources, ['x.js'], 'the source map points at the input file');
  console.log('  [PASS] karma adapter forwards coverageVariable and produceSourceMap');
}

{
  const source = 'if (a) { b(); } else { c(); }';
  const armZero = (instrumenter) => {
    instrumenter.instrumentSync(source, 'if.js');
    const branch = Object.values(instrumenter.lastFileCoverage().branchMap).find(
      (entry) => entry.type === 'if',
    );
    const loc = branch.locations[0];
    return [loc.start.line, loc.start.column, loc.end.line, loc.end.column];
  };
  assert.deepEqual(armZero(new KarmaInstrumenter({})), [1, 0, 1, 29], 'karma default arm 0');
  assert.deepEqual(
    armZero(new KarmaInstrumenter({ ifConsequentLocation: true })),
    [1, 7, 1, 15],
    'karma adapter forwards ifConsequentLocation',
  );
  console.log('  [PASS] karma adapter forwards ifConsequentLocation');
}

{
  // A filename-less call puts the callback in the second argument, as in
  // istanbul-lib-instrument.
  const oxc = new KarmaInstrumenter();
  let result;
  oxc.instrument('var y = 2;', (error, code) => {
    assert.equal(error, null);
    result = code;
  });
  assert.equal(typeof result, 'string', 'instrument(code, callback) works');

  let parseError;
  oxc.instrument('var = ;', 'broken.js', (error, code) => {
    parseError = error;
    assert.equal(code, undefined, 'no code on error');
  });
  assert(parseError instanceof Error, 'a parse error goes to the callback');

  // An error that the callback throws must not reach the callback again.
  let errorCalls = 0;
  assert.throws(
    () =>
      oxc.instrument('var z = 3;', 'z.js', (error) => {
        if (error) errorCalls += 1;
        throw new Error('callback failure');
      }),
    /callback failure/,
  );
  assert.equal(errorCalls, 0, 'a throwing callback is not called a second time with its error');

  assert.throws(() => oxc.instrument('var a;', 'a.js'), TypeError, 'a missing callback throws');
  assert.throws(
    () => new KarmaInstrumenter({ produceSourceMap: 'yes' }),
    TypeError,
    'a non-boolean produceSourceMap throws',
  );
  console.log('  [PASS] karma adapter callback and error contract');
}

{
  // An input source map from an earlier karma preprocessor composes through.
  const oxc = new KarmaInstrumenter({ produceSourceMap: true });
  const inputMap = {
    version: 3,
    sources: ['original.js'],
    names: [],
    mappings: 'AAAA',
    sourcesContent: ['var q = 1;'],
  };
  oxc.instrumentSync('var q = 1;', 'built.js', inputMap);
  assert.deepEqual(oxc.lastSourceMap().sources, ['original.js'], 'output map composes the input map');
  assert.deepEqual(
    oxc.lastFileCoverage().inputSourceMap,
    inputMap,
    'file coverage keeps the input source map for the karma reporter',
  );
  console.log('  [PASS] karma adapter composes an input source map');
}

{
  // karma-coverage matches `coverageReporter.instrumenter` patterns against
  // the absolute path of each file. A relative pattern such as 'src/**/*.js'
  // never matches, and karma-coverage then uses istanbul without a warning.
  // Every documented example must therefore start its pattern with `**/`.
  const documents = [
    'karma.js',
    'karma.d.ts',
    'README.md',
    '../../README.md',
    '../../examples/karma/README.md',
  ];
  for (const document of documents) {
    const text = readFileSync(join(__dirname, document), 'utf8').replaceAll('\\/', '/');
    const patterns = [...text.matchAll(/\binstrumenter: \{([^}]*)\}/g)].flatMap(([, body]) =>
      [...body.matchAll(/'([^']+)'\s*:/g)].map(([, pattern]) => pattern),
    );
    assert(patterns.length > 0, `${document} has an instrumenter example`);
    for (const pattern of patterns) {
      assert(pattern.startsWith('**/'), `${document}: instrumenter pattern '${pattern}' must start with **/`);
    }
  }
  console.log('  [PASS] karma instrumenter examples match absolute paths');
}

// The Babel plugin has its own file; it runs here so that every runner of
// test.mjs also runs it.
console.log('');
await import('./babel.test.mjs');

console.log('\nAll tests passed!');
