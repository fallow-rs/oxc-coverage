#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const rootDir = fileURLToPath(new URL('..', import.meta.url));
const napiDir = 'crates/oxc_coverage_instrument_napi';
const platformDir = `${napiDir}/npm`;

const targetPackageEntries = [
  ['aarch64-apple-darwin', 'darwin-arm64'],
  ['x86_64-apple-darwin', 'darwin-x64'],
  ['aarch64-unknown-linux-gnu', 'linux-arm64-gnu'],
  ['x86_64-unknown-linux-gnu', 'linux-x64-gnu'],
  ['x86_64-unknown-linux-musl', 'linux-x64-musl'],
  ['wasm32-wasip1-threads', 'wasm32-wasi'],
  ['wasm32-wasip1', 'wasm32-wasi-singlethreaded'],
  ['aarch64-pc-windows-msvc', 'win32-arm64-msvc'],
  ['x86_64-pc-windows-msvc', 'win32-x64-msvc'],
];
const targetPackages = new Map(targetPackageEntries);
const packageName = (packageDir) => `@oxc-coverage-instrument/binding-${packageDir}`;

const platformPackSpecs = [
  { packageDir: 'darwin-arm64', files: ['package/coverage-instrument.darwin-arm64.node'] },
  { packageDir: 'darwin-x64', files: ['package/coverage-instrument.darwin-x64.node'] },
  {
    packageDir: 'linux-arm64-gnu',
    files: ['package/coverage-instrument.linux-arm64-gnu.node'],
  },
  { packageDir: 'linux-x64-gnu', files: ['package/coverage-instrument.linux-x64-gnu.node'] },
  { packageDir: 'linux-x64-musl', files: ['package/coverage-instrument.linux-x64-musl.node'] },
  {
    packageDir: 'wasm32-wasi',
    requiresLocalArtifacts: true,
    files: [
      'package/coverage-instrument.wasm32-wasi.wasm',
      'package/coverage-instrument.wasi.cjs',
      'package/coverage-instrument.wasi-browser.js',
      'package/wasi-worker.mjs',
      'package/wasi-worker-browser.mjs',
    ],
  },
  {
    packageDir: 'wasm32-wasi-singlethreaded',
    requiresLocalArtifacts: true,
    files: [
      'package/coverage-instrument.wasm32-wasi.wasm',
      'package/coverage-instrument.wasi.cjs',
      'package/coverage-instrument.wasi-browser.js',
    ],
  },
  {
    packageDir: 'win32-arm64-msvc',
    files: ['package/coverage-instrument.win32-arm64-msvc.node'],
  },
  { packageDir: 'win32-x64-msvc', files: ['package/coverage-instrument.win32-x64-msvc.node'] },
];

const args = process.argv.slice(2);
const metadataOnly = args.length === 1 && args[0] === '--metadata-only';
const requireReleaseArtifacts =
  args.length === 1 && args[0] === '--require-release-artifacts';
if (args.length > 0 && !metadataOnly && !requireReleaseArtifacts) {
  throw new Error(`Unknown argument: ${args.join(' ')}`);
}

const rootManifestPath = `${napiDir}/package.json`;
const rootManifest = JSON.parse(readFileSync(resolve(rootDir, rootManifestPath), 'utf8'));
const expectedTargets = new Set(targetPackages.keys());
const expectedPackages = new Set([...targetPackages.values()].map(packageName));

const contractErrors = [];
const checkUnique = (label, values) => {
  const seen = new Set();
  const duplicates = new Set();
  for (const value of values) {
    if (seen.has(value)) duplicates.add(value);
    seen.add(value);
  }
  if (duplicates.size > 0) {
    contractErrors.push(`duplicate ${label}: ${[...duplicates].sort().join(', ')}`);
  }
};
const checkSet = (label, expected, actual) => {
  const missing = [...expected].filter((value) => !actual.has(value)).sort();
  const unexpected = [...actual].filter((value) => !expected.has(value)).sort();
  if (missing.length === 0 && unexpected.length === 0) {
    return;
  }
  const details = [];
  if (missing.length > 0) details.push(`missing ${missing.join(', ')}`);
  if (unexpected.length > 0) details.push(`unexpected ${unexpected.join(', ')}`);
  contractErrors.push(`${label}: ${details.join('; ')}`);
};

const findYamlBlock = (lines, parent, key, indent) => {
  const header = `${' '.repeat(indent)}${key}:`;
  const headerIndex = lines.findIndex(
    (line, index) => index >= parent.start && index < parent.end && line === header,
  );
  if (headerIndex === -1) return { start: parent.end, end: parent.end };

  let end = parent.end;
  for (let index = headerIndex + 1; index < parent.end; index += 1) {
    const line = lines[index];
    const trimmed = line.trimStart();
    if (trimmed === '' || trimmed.startsWith('#')) continue;
    const lineIndent = line.length - trimmed.length;
    if (lineIndent <= indent) {
      end = index;
      break;
    }
  }
  return { start: headerIndex + 1, end };
};

const releaseBuildTargets = (workflow) => {
  const lines = workflow.split(/\r?\n/);
  const root = { start: 0, end: lines.length };
  const jobs = findYamlBlock(lines, root, 'jobs', 0);
  const build = findYamlBlock(lines, jobs, 'build', 2);
  const strategy = findYamlBlock(lines, build, 'strategy', 4);
  const matrix = findYamlBlock(lines, strategy, 'matrix', 6);
  const include = findYamlBlock(lines, matrix, 'include', 8);
  const prefix = `${' '.repeat(12)}target:`;
  return lines
    .slice(include.start, include.end)
    .filter((line) => line.startsWith(prefix))
    .map((line) => line.slice(prefix.length).trim().split(/\s+#/, 1)[0]);
};

checkUnique(
  'canonical target triples',
  targetPackageEntries.map(([target]) => target),
);
checkUnique(
  'canonical package directories',
  targetPackageEntries.map(([, packageDir]) => packageDir),
);
// napi-rs 3.9 gives wasm32-wasip1 its own package layout. This repo builds and
// packages that target with its own scripts, so napi.targets must omit it.
const repoPackagedTargets = new Set(['wasm32-wasip1']);
const expectedNapiTargets = new Set(
  [...expectedTargets].filter((target) => !repoPackagedTargets.has(target)),
);
const napiTargets = rootManifest.napi?.targets ?? [];
checkUnique('napi.targets', napiTargets);
checkSet('napi.targets', expectedNapiTargets, new Set(napiTargets));
checkSet(
  'optionalDependencies',
  expectedPackages,
  new Set(
    Object.keys(rootManifest.optionalDependencies ?? {}).filter((name) =>
      name.startsWith('@oxc-coverage-instrument/binding-'),
    ),
  ),
);

const platformManifests = readdirSync(resolve(rootDir, platformDir), {
  withFileTypes: true,
})
  .filter((entry) => entry.isDirectory())
  .map((entry) => `${platformDir}/${entry.name}/package.json`)
  .filter((manifest) => existsSync(resolve(rootDir, manifest)))
  .sort();

const platformManifestRecords = platformManifests.map((manifest) => {
  const packageJson = JSON.parse(readFileSync(resolve(rootDir, manifest), 'utf8'));
  const packageDir = manifest.slice(
    `${platformDir}/`.length,
    -'/package.json'.length,
  );
  return { packageDir, packageName: packageJson.name };
});
const platformPackageNames = platformManifestRecords.map((record) => record.packageName);
checkUnique('platform manifest names', platformPackageNames);
const platformMappingErrors = [];
for (const record of platformManifestRecords) {
  const expectedName = packageName(record.packageDir);
  if (!expectedPackages.has(expectedName)) {
    platformMappingErrors.push(`${record.packageDir} is not a registered package directory`);
  } else if (record.packageName !== expectedName) {
    platformMappingErrors.push(
      `${record.packageDir} expected ${expectedName}, got ${record.packageName}`,
    );
  }
}
if (platformMappingErrors.length > 0) {
  contractErrors.push(`platform directory mappings: ${platformMappingErrors.join('; ')}`);
}
const platformPackages = new Set(platformPackageNames);
checkSet('platform manifests', expectedPackages, platformPackages);

const releaseWorkflow = readFileSync(resolve(rootDir, '.github/workflows/release-npm.yml'), 'utf8');
const releaseTargetList = releaseBuildTargets(releaseWorkflow);
checkUnique('release targets', releaseTargetList);
const releaseTargets = new Set(releaseTargetList);
checkSet('release targets', expectedTargets, releaseTargets);

const packPackageList = platformPackSpecs.map((spec) => packageName(spec.packageDir));
checkUnique('pack specs', packPackageList);
const packPackages = new Set(packPackageList);
checkSet('pack specs', expectedPackages, packPackages);

if (contractErrors.length > 0) {
  throw new Error(`Invalid npm target contracts:\n${contractErrors.join('\n')}`);
}

const manifests = [
  { path: rootManifestPath, expectedDirectory: napiDir },
  ...platformManifests.map((path) => ({
    path,
    expectedDirectory: path.slice(0, -'/package.json'.length),
  })),
];

const metadataErrors = [];
for (const manifest of manifests) {
  const packageJson = JSON.parse(readFileSync(resolve(rootDir, manifest.path), 'utf8'));
  const actualDirectory = packageJson.repository?.directory;
  if (actualDirectory !== manifest.expectedDirectory) {
    const expected = JSON.stringify(manifest.expectedDirectory);
    const actual = JSON.stringify(actualDirectory);
    metadataErrors.push(
      `${manifest.path}: expected repository.directory ${expected}, got ${actual}`,
    );
  }
}

if (metadataErrors.length > 0) {
  throw new Error(`Invalid npm repository metadata:\n${metadataErrors.join('\n')}`);
}
console.log(`npm repository metadata OK: ${manifests.length} manifests`);
console.log(`npm target contracts OK: ${expectedTargets.size} targets`);

if (metadataOnly) {
  process.exit(0);
}

const specs = [
  {
    dir: 'crates/oxc_coverage_instrument_napi',
    files: [
      'package/browser.js',
      'package/index.js',
      'package/index.d.ts',
      'package/coverage-instrument.wasi.cjs',
      'package/coverage-instrument.wasi-browser.js',
      'package/wasi-worker.mjs',
      'package/wasi-worker-browser.mjs',
      'package/vitest.js',
      'package/vitest.d.ts',
    ],
  },
  ...platformPackSpecs.map((spec) => ({
    dir: `${platformDir}/${spec.packageDir}`,
    files:
      requireReleaseArtifacts || spec.requiresLocalArtifacts === true ? spec.files : [],
  })),
];

for (const spec of specs) {
  const result = spawnSync('npm', ['pack', '--dry-run', '--json'], {
    cwd: resolve(rootDir, spec.dir),
    encoding: 'utf8',
  });
  if (result.status !== 0) {
    process.stderr.write(result.stderr);
    throw new Error(`npm pack failed in ${spec.dir}`);
  }

  // npm 10 prints an array. npm 12 prints an object keyed by package name.
  const packed = JSON.parse(result.stdout);
  const pack = Array.isArray(packed) ? packed[0] : Object.values(packed)[0];
  const actual = new Set(pack.files.map((file) => file.path.replace(/^package\//, '')));
  const missing = spec.files.filter((file) => !actual.has(file.replace(/^package\//, '')));
  if (missing.length > 0) {
    throw new Error(`${spec.dir} package is missing files: ${missing.join(', ')}`);
  }
  console.log(`npm pack surface OK: ${spec.dir}`);
}
