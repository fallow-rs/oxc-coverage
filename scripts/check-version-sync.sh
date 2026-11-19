#!/usr/bin/env bash
# Keep workspace crate versions in sync and catch the lagged-delta publish trap.
#
# Modes:
#   --mode=pins  (default, deterministic, no network)
#       1. Every internal `oxc_coverage_* = { path = .., version = "X" }` pin
#          equals that dependency crate's local [package].version.
#       2. Every publishable crate (no `publish = false`) appears in the
#          publish-crate job of .github/workflows/release-npm.yml.
#     Catches drift like a consumer pinning source_maps "0.3.0" while the crate
#     is locally 0.3.2, or a new publishable crate missing from the release
#     workflow.
#
#   --mode=published  (release-time, networked)
#       For each publishable crate whose local version is ALREADY published on
#       crates.io, diff the local src/ against the published .crate's src/. Any
#       drift means the crate changed without a version bump, so the release
#       would ship code that does not match what the version already promises,
#       while crates.io refuses to overwrite it. Fail loudly here instead and
#       bump the crate first.
#
# Usage:
#   scripts/check-version-sync.sh [--mode=pins|published]
set -euo pipefail

MODE="pins"
for arg in "$@"; do
  case "$arg" in
    --mode=*) MODE="${arg#--mode=}" ;;
    -h|--help) sed -n '2,30p' "$0"; exit 0 ;;
    *) echo "unknown arg: $arg (try --mode=pins or --mode=published)" >&2; exit 2 ;;
  esac
done

# Resolve repo root from the script location (scripts/ lives at the root), so
# the checks run the same regardless of cwd or whether git is available.
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

python3 - "$MODE" <<'PY'
import sys, tomllib, re, pathlib, urllib.request, urllib.error, io, tarfile, tempfile, filecmp, json

mode = sys.argv[1]
root = pathlib.Path.cwd()
crates_dir = root / "crates"

# --- Load every workspace crate manifest -----------------------------------
crates = {}  # cargo_name -> {version, publish, dir, path}
for manifest in sorted(crates_dir.glob("*/Cargo.toml")):
    data = tomllib.loads(manifest.read_text())
    pkg = data.get("package", {})
    name = pkg.get("name")
    if not name:
        continue
    crates[name] = {
        "version": pkg.get("version"),
        "publish": pkg.get("publish", True),  # default True == publishable
        "dir": manifest.parent.name,
        "deps": {**data.get("dependencies", {}),
                 **data.get("dev-dependencies", {}),
                 **data.get("build-dependencies", {})},
    }

dir_to_name = {c["dir"]: n for n, c in crates.items()}
failures = []
napi_dir = root / "crates/oxc_coverage_instrument_napi"


def load_json(path):
    try:
        return json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as error:
        failures.append(f"{path.relative_to(root)}: cannot read JSON: {error}")
        return {}


def expect_version(label, actual, expected):
    if actual != expected:
        failures.append(f'{label} "{actual}" != public Rust version "{expected}"')


def check_public_npm_versions():
    public_version = crates.get("oxc_coverage_instrument", {}).get("version")
    if public_version is None:
        failures.append("public crate oxc_coverage_instrument is missing")
        return

    manifest = load_json(napi_dir / "package.json")
    expect_version("published npm package version", manifest.get("version"), public_version)

    platform_dir = napi_dir / "npm"
    for path in sorted(platform_dir.glob("*/package.json")):
        package = load_json(path)
        label = f"{path.parent.name} platform package version"
        expect_version(label, package.get("version"), public_version)

    prefix = "@oxc-coverage-instrument/"
    manifest_optional = {
        name: version
        for name, version in manifest.get("optionalDependencies", {}).items()
        if name.startswith(prefix)
    }
    for name, version in sorted(manifest_optional.items()):
        expect_version(f"published npm optional dependency {name}", version, public_version)

    lockfile = load_json(napi_dir / "package-lock.json")
    expect_version("lockfile top-level version", lockfile.get("version"), public_version)
    lock_packages = lockfile.get("packages", {})
    lock_root = lock_packages.get("", {})
    expect_version("lockfile root package version", lock_root.get("version"), public_version)
    lock_optional = {
        name: version
        for name, version in lock_root.get("optionalDependencies", {}).items()
        if name.startswith(prefix)
    }
    for name in sorted(manifest_optional):
        expect_version(
            f"lockfile optional dependency {name}",
            lock_optional.get(name),
            public_version,
        )
        package = lock_packages.get(f"node_modules/{name}", {})
        expect_version(
            f"lockfile package {name} version",
            package.get("version"),
            public_version,
        )
    for name in sorted(set(lock_optional) - set(manifest_optional)):
        failures.append(f"lockfile optional dependency {name} is absent from package.json")

    check_platform_emnapi_versions(manifest)


def check_platform_emnapi_versions(manifest):
    # `napi pre-publish` rejects a WASI platform package whose emnapi runtime
    # dependencies differ from the root package, after the crates.io publish.
    root_versions = {
        **manifest.get("devDependencies", {}),
        **manifest.get("dependencies", {}),
    }
    for path in sorted((napi_dir / "npm").glob("*/package.json")):
        package = load_json(path)
        for name, version in sorted(package.get("dependencies", {}).items()):
            if not (name == "emnapi" or name.startswith("@emnapi/")):
                continue
            expected = root_versions.get(name)
            if version != expected:
                failures.append(
                    f'{path.parent.name} platform package dependency {name} "{version}" '
                    f'!= root package version "{expected}"'
                )


def check_pins():
    # 1. internal version pins == dependency crate's local version
    for name, c in crates.items():
        for dep_name, spec in c["deps"].items():
            if not dep_name.startswith("oxc_coverage_"):
                continue
            if not isinstance(spec, dict):
                continue  # a bare "x.y.z" string would be a non-path dep; n/a here
            path = spec.get("path")
            version = spec.get("version")
            if path is None:
                continue
            target_dir = pathlib.PurePosixPath(path).name  # ../oxc_coverage_x -> oxc_coverage_x
            target_name = dir_to_name.get(target_dir)
            if target_name is None:
                failures.append(f"{name}: dep '{dep_name}' path '{path}' resolves to no workspace crate")
                continue
            target_version = crates[target_name]["version"]
            if version is None:
                # Path-only dep (no version). Only acceptable when THIS crate is
                # not published to crates.io (path deps need no version pin then).
                if c["publish"] is False:
                    continue
                failures.append(
                    f"{name} (publishable) pins '{dep_name}' by path with no version; "
                    f"add version = \"{target_version}\""
                )
                continue
            if version != target_version:
                failures.append(
                    f"{name}: pin '{dep_name} = \"{version}\"' != local "
                    f"{target_name} version \"{target_version}\" (update the pin)"
                )

    # 2. publish-completeness: every publishable crate is in the release workflow
    wf = (root / ".github/workflows/release-npm.yml").read_text()
    published_in_wf = set(re.findall(r"cargo publish -p (\w+)", wf))
    for name, c in crates.items():
        if c["publish"] is False:
            continue
        if name not in published_in_wf:
            failures.append(
                f"{name} is publishable but has no `cargo publish -p {name}` "
                f"step in release-npm.yml (add it or set publish = false)"
            )

    check_public_npm_versions()


def index_url(name):
    # crates.io sparse-index path: ab/cd/name for >=4 chars
    n = name.lower()
    if len(n) == 1:
        return f"https://index.crates.io/1/{n}"
    if len(n) == 2:
        return f"https://index.crates.io/2/{n}"
    if len(n) == 3:
        return f"https://index.crates.io/3/{n[0]}/{n}"
    return f"https://index.crates.io/{n[0:2]}/{n[2:4]}/{n}"


def published_versions(name):
    try:
        with urllib.request.urlopen(index_url(name), timeout=30) as r:
            body = r.read().decode()
    except urllib.error.HTTPError as e:
        if e.code == 404:
            return set()  # never published
        raise
    return {json.loads(line)["vers"] for line in body.splitlines() if line.strip()}


def dir_differs(local_src, published_src):
    """Recursively compare two src trees; return list of differing/missing rels."""
    diffs = []
    cmp = filecmp.dircmp(str(local_src), str(published_src))

    def walk(c, prefix=""):
        for f in c.left_only:
            diffs.append(f"{prefix}{f} (only local)")
        for f in c.right_only:
            diffs.append(f"{prefix}{f} (only published)")
        for f in c.diff_files:
            diffs.append(f"{prefix}{f} (content differs)")
        for sub, subc in c.subdirs.items():
            walk(subc, prefix + sub + "/")

    walk(cmp)
    return diffs


def check_published():
    for name, c in crates.items():
        if c["publish"] is False:
            continue
        version = c["version"]
        pub = published_versions(name)
        if version not in pub:
            print(f"  - {name} {version}: not yet on crates.io (pending publish) - OK")
            continue
        # The local version is already published. Compare src/ to detect a
        # changed-without-bump (lagged-delta) crate.
        url = f"https://static.crates.io/crates/{name}/{name}-{version}.crate"
        try:
            with urllib.request.urlopen(url, timeout=60) as r:
                blob = r.read()
        except urllib.error.HTTPError as e:
            failures.append(f"{name} {version}: cannot fetch published .crate ({e.code}) from {url}")
            continue
        with tempfile.TemporaryDirectory() as td:
            with tarfile.open(fileobj=io.BytesIO(blob), mode="r:gz") as tf:
                tf.extractall(td, filter="data")
            pub_src = pathlib.Path(td) / f"{name}-{version}" / "src"
            local_src = crates_dir / c["dir"] / "src"
            if not pub_src.exists() or not local_src.exists():
                failures.append(f"{name} {version}: missing src/ for comparison")
                continue
            diffs = dir_differs(local_src, pub_src)
            if diffs:
                failures.append(
                    f"{name} {version} is already on crates.io but local src/ differs "
                    f"(bump {name} before releasing): " + ", ".join(diffs[:8])
                    + (" ..." if len(diffs) > 8 else "")
                )
            else:
                print(f"  - {name} {version}: src matches published - OK")


if mode == "pins":
    print("version-sync: checking internal pins + publish-completeness")
    check_pins()
elif mode == "published":
    print("version-sync: checking published crates for unbumped src drift")
    check_published()
else:
    print(f"unknown mode: {mode}", file=sys.stderr)
    sys.exit(2)

if failures:
    print(f"\nversion-sync FAILED ({len(failures)} issue(s)):", file=sys.stderr)
    for f in failures:
        print(f"  ✗ {f}", file=sys.stderr)
    sys.exit(1)

print("version-sync OK")
PY
