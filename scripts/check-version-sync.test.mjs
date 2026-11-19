import assert from 'node:assert/strict';
import {
  chmodSync,
  copyFileSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const scratch = mkdtempSync(resolve(tmpdir(), 'oxc-version-sync-'));
const publicVersion = '0.10.1';
const optionalName = '@oxc-coverage-instrument/binding-darwin-arm64';
const optionalPackagePath = `node_modules/${optionalName}`;

const writeJson = (path, value) => {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`);
};

const readJson = (path) => JSON.parse(readFileSync(path, 'utf8'));

const makeFixture = (version = publicVersion) => {
  const root = mkdtempSync(resolve(scratch, 'fixture-'));
  mkdirSync(resolve(root, 'scripts'), { recursive: true });
  copyFileSync(resolve(repo, 'scripts/check-version-sync.sh'), resolve(root, 'scripts/check-version-sync.sh'));
  copyFileSync(resolve(repo, 'scripts/sync-npm-versions.sh'), resolve(root, 'scripts/sync-npm-versions.sh'));
  chmodSync(resolve(root, 'scripts/check-version-sync.sh'), 0o755);
  chmodSync(resolve(root, 'scripts/sync-npm-versions.sh'), 0o755);

  mkdirSync(resolve(root, 'crates/oxc_coverage_instrument'), { recursive: true });
  writeFileSync(
    resolve(root, 'crates/oxc_coverage_instrument/Cargo.toml'),
    `[package]\nname = "oxc_coverage_instrument"\nversion = "${version}"\n`,
  );
  mkdirSync(resolve(root, 'crates/oxc_coverage_instrument_napi'), { recursive: true });
  writeFileSync(
    resolve(root, 'crates/oxc_coverage_instrument_napi/Cargo.toml'),
    '[package]\nname = "oxc_coverage_instrument_napi"\nversion = "0.2.0"\npublish = false\n',
  );
  mkdirSync(resolve(root, '.github/workflows'), { recursive: true });
  writeFileSync(
    resolve(root, '.github/workflows/release-npm.yml'),
    'jobs:\n  publish-crate:\n    steps:\n      - run: cargo publish -p oxc_coverage_instrument\n',
  );

  const optionalDependencies = { [optionalName]: version };
  writeJson(resolve(root, 'crates/oxc_coverage_instrument_napi/package.json'), {
    name: 'oxc-coverage-instrument',
    version,
    optionalDependencies,
  });
  writeJson(resolve(root, 'crates/oxc_coverage_instrument_napi/npm/darwin-arm64/package.json'), {
    name: optionalName,
    version,
  });
  writeJson(resolve(root, 'crates/oxc_coverage_instrument_napi/package-lock.json'), {
    name: 'oxc-coverage-instrument',
    version,
    lockfileVersion: 3,
    packages: {
      '': {
        name: 'oxc-coverage-instrument',
        version,
        optionalDependencies,
      },
      [optionalPackagePath]: {
        version,
        resolved:
          `https://registry.npmjs.org/${optionalName}/-/${optionalName.split('/')[1]}-${version}.tgz`,
        integrity: 'sha512-published-version-integrity',
        cpu: ['arm64'],
        optional: true,
        os: ['darwin'],
      },
    },
  });
  const gitInit = spawnSync('git', ['init', '-q'], { cwd: root, encoding: 'utf8' });
  assert.equal(gitInit.status, 0, gitInit.stderr);
  return root;
};

const runCheck = (root, expectedVersion = null) => {
  if (expectedVersion !== null) {
    writeFileSync(
      resolve(root, 'crates/oxc_coverage_instrument/Cargo.toml'),
      `[package]\nname = "oxc_coverage_instrument"\nversion = "${expectedVersion}"\n`,
    );
  }
  return spawnSync(resolve(root, 'scripts/check-version-sync.sh'), ['--mode=pins'], {
    cwd: root,
    encoding: 'utf8',
  });
};

const mutateJson = (root, relativePath, mutate) => {
  const path = resolve(root, relativePath);
  const value = readJson(path);
  mutate(value);
  writeJson(path, value);
};

const expectDriftFailure = (name, relativePath, mutate, expected) => {
  const root = makeFixture();
  mutateJson(root, relativePath, mutate);
  const result = runCheck(root);
  assert.notEqual(result.status, 0, `${name} was accepted`);
  assert.match(`${result.stdout}\n${result.stderr}`, expected, name);
};

try {
  assert.equal(runCheck(makeFixture()).status, 0, 'synchronized fixture must pass');
  expectDriftFailure(
    'published npm version drift',
    'crates/oxc_coverage_instrument_napi/package.json',
    (manifest) => { manifest.version = '0.10.0'; },
    /published npm package version/,
  );
  expectDriftFailure(
    'platform package version drift',
    'crates/oxc_coverage_instrument_napi/npm/darwin-arm64/package.json',
    (manifest) => { manifest.version = '0.10.0'; },
    /darwin-arm64.*version/,
  );
  expectDriftFailure(
    'published npm optional pin drift',
    'crates/oxc_coverage_instrument_napi/package.json',
    (manifest) => { manifest.optionalDependencies[optionalName] = '0.10.0'; },
    /optional dependency.*darwin-arm64/,
  );
  expectDriftFailure(
    'lockfile top-level version drift',
    'crates/oxc_coverage_instrument_napi/package-lock.json',
    (lockfile) => { lockfile.version = '0.10.0'; },
    /lockfile top-level version/,
  );
  expectDriftFailure(
    'lockfile root package version drift',
    'crates/oxc_coverage_instrument_napi/package-lock.json',
    (lockfile) => { lockfile.packages[''].version = '0.10.0'; },
    /lockfile root package version/,
  );
  expectDriftFailure(
    'lockfile optional pin drift',
    'crates/oxc_coverage_instrument_napi/package-lock.json',
    (lockfile) => { lockfile.packages[''].optionalDependencies[optionalName] = '0.10.0'; },
    /lockfile optional dependency.*darwin-arm64/,
  );
  expectDriftFailure(
    'lockfile missing optional pin',
    'crates/oxc_coverage_instrument_napi/package-lock.json',
    (lockfile) => { delete lockfile.packages[''].optionalDependencies[optionalName]; },
    /lockfile optional dependency.*darwin-arm64/,
  );
  expectDriftFailure(
    'lockfile extra optional pin',
    'crates/oxc_coverage_instrument_napi/package-lock.json',
    (lockfile) => {
      lockfile.packages[''].optionalDependencies[
        '@oxc-coverage-instrument/binding-linux-x64-gnu'
      ] = publicVersion;
    },
    /lockfile optional dependency.*linux-x64-gnu.*absent from package\.json/,
  );
  expectDriftFailure(
    'lockfile installed optional package version drift',
    'crates/oxc_coverage_instrument_napi/package-lock.json',
    (lockfile) => { lockfile.packages[optionalPackagePath].version = '0.10.0'; },
    /lockfile package.*darwin-arm64.*version/,
  );
  const emnapiRoot = makeFixture();
  mutateJson(emnapiRoot, 'crates/oxc_coverage_instrument_napi/package.json', (manifest) => {
    manifest.dependencies = { '@emnapi/core': '2.0.0-beta.3' };
  });
  writeJson(resolve(emnapiRoot, 'crates/oxc_coverage_instrument_napi/npm/wasm32-wasi/package.json'), {
    name: '@oxc-coverage-instrument/binding-wasm32-wasi',
    version: publicVersion,
    dependencies: { '@emnapi/core': '2.0.0-beta.3' },
  });
  assert.equal(runCheck(emnapiRoot).status, 0, 'matching WASI emnapi pins must pass');
  mutateJson(emnapiRoot, 'crates/oxc_coverage_instrument_napi/npm/wasm32-wasi/package.json', (manifest) => {
    manifest.dependencies['@emnapi/core'] = '2.0.0-beta.2';
  });
  const emnapiResult = runCheck(emnapiRoot);
  assert.notEqual(emnapiResult.status, 0, 'WASI emnapi pin drift was accepted');
  assert.match(
    `${emnapiResult.stdout}\n${emnapiResult.stderr}`,
    /wasm32-wasi platform package dependency @emnapi\/core "2\.0\.0-beta\.2"/,
  );
  const syncRoot = makeFixture();
  const syncResult = spawnSync(
    resolve(syncRoot, 'scripts/sync-npm-versions.sh'),
    [publicVersion, '0.10.2'],
    { cwd: syncRoot, encoding: 'utf8' },
  );
  assert.equal(syncResult.status, 0, `${syncResult.stdout}\n${syncResult.stderr}`);
  const syncedCheck = runCheck(syncRoot, '0.10.2');
  assert.equal(syncedCheck.status, 0, `${syncedCheck.stdout}\n${syncedCheck.stderr}`);
  const syncedLock = readJson(
    resolve(syncRoot, 'crates/oxc_coverage_instrument_napi/package-lock.json'),
  );
  assert.equal(syncedLock.packages[optionalPackagePath].version, '0.10.2');
  assert.match(syncedLock.packages[optionalPackagePath].resolved, /0\.10\.2\.tgz$/);
  assert.equal('integrity' in syncedLock.packages[optionalPackagePath], false);
  console.log('version sync fixture tests: PASS');
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
