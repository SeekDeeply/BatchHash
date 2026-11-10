#!/usr/bin/env node
// Version / tag consistency gate for CI and the release workflow.
//
// Reads the workspace Cargo version, the Flutter app version, the top
// CHANGELOG heading and (when a tag is being built) the pushed tag, then exits
// non-zero on any mismatch. No dependencies beyond Node's standard library.
//
// Usage:
//   node scripts/verify-versions.mjs
//   node scripts/verify-versions.mjs --tag v1.0.0
//
// In GitHub Actions the tag is read from GITHUB_REF / GITHUB_REF_NAME. A branch
// push has no tag, so only cargo/pubspec/changelog consistency is checked, which
// keeps ordinary development commits green.

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), '..');

function fail(message) {
  console.error(`verify-versions: ${message}`);
  process.exitCode = 1;
}

function read(relativePath) {
  return readFileSync(join(repoRoot, relativePath), 'utf8');
}

function cargoWorkspaceVersion() {
  const cargo = read('Cargo.toml');
  const section = cargo.match(/\[workspace\.package\]([\s\S]*?)(?=\n\[|$)/);
  if (!section) {
    fail('Cargo.toml has no [workspace.package] section');
    return null;
  }
  const version = section[1].match(/^\s*version\s*=\s*"([^"]+)"/m);
  if (!version) {
    fail('Cargo.toml [workspace.package] has no version');
    return null;
  }
  return version[1];
}

function pubspecVersion() {
  const pubspec = read('app/pubspec.yaml');
  const version = pubspec.match(/^version:\s*(\S+)\s*$/m);
  if (!version) {
    fail('app/pubspec.yaml has no version: field');
    return null;
  }
  // Flutter build metadata (the +N suffix) is not part of the release version.
  return version[1].split('+')[0];
}

function changelog() {
  const text = read('CHANGELOG.md');
  const headings = [...text.matchAll(/^##\s+\[([^\]]+)\]/gm)].map((m) =>
    m[1].trim(),
  );
  if (headings.length === 0) {
    fail('CHANGELOG.md has no "## [version]" headings');
    return null;
  }
  const unreleasedPattern = /^(未发布|unreleased)$/i;
  return {
    top: headings[0],
    unreleased: unreleasedPattern.test(headings[0]),
    released: headings.filter((heading) => !unreleasedPattern.test(heading)),
  };
}

function requestedTag() {
  const argv = process.argv.slice(2);
  const flag = argv.indexOf('--tag');
  if (flag !== -1 && argv[flag + 1]) {
    return argv[flag + 1];
  }
  const positional = argv.find((arg) => !arg.startsWith('--'));
  if (positional) {
    return positional;
  }
  const ref = process.env.GITHUB_REF ?? '';
  if (ref.startsWith('refs/tags/')) {
    return ref.slice('refs/tags/'.length);
  }
  if (process.env.GITHUB_REF_TYPE === 'tag' && process.env.GITHUB_REF_NAME) {
    return process.env.GITHUB_REF_NAME;
  }
  return null;
}

// The Rust toolchain is pinned in two places that must agree: rust-toolchain.toml
// (which rustup honours inside the repository) and the dtolnay/rust-toolchain
// action in every workflow. Drift between them is silent and expensive — extra
// targets such as the Android ones are installed per channel, so a mismatch
// breaks the packaging job with a confusing "can't find crate for core".
function rustToolchainChannel() {
  const text = read('rust-toolchain.toml');
  const channel = text.match(/^\s*channel\s*=\s*"([^"]+)"/m);
  if (!channel) {
    fail('rust-toolchain.toml has no channel');
    return null;
  }
  return channel[1];
}

function workflowToolchainPins() {
  const workflows = ['ci.yml', 'release.yml', 'format.yml', 'release-check.yml'];
  const pins = new Map();
  for (const name of workflows) {
    let text;
    try {
      text = read('.github/workflows/' + name);
    } catch {
      continue;
    }
    for (const match of text.matchAll(/dtolnay\/rust-toolchain@([\w.]+)/g)) {
      const channel = match[1];
      const list = pins.get(channel) ?? [];
      if (!list.includes(name)) {
        list.push(name);
      }
      pins.set(channel, list);
    }
  }
  return pins;
}

function rustVersionFloor() {
  const cargo = read('Cargo.toml');
  const section = cargo.match(/\[workspace\.package\]([\s\S]*?)(?=\n\[|$)/);
  const version = section && section[1].match(/^\s*rust-version\s*=\s*"([^"]+)"/m);
  if (!version) {
    fail('Cargo.toml [workspace.package] has no rust-version');
    return null;
  }
  return version[1];
}

// Compare two Rust versions as far as they are specified: 1.89 vs 1.89.0 are
// equal, and "1.82" is below "1.89.0".
function compareRustVersions(left, right) {
  const parts = (value) => value.split('.').map((piece) => Number(piece) || 0);
  const a = parts(left);
  const b = parts(right);
  for (let index = 0; index < 3; index += 1) {
    const delta = (a[index] ?? 0) - (b[index] ?? 0);
    if (delta !== 0) {
      return delta < 0 ? -1 : 1;
    }
  }
  return 0;
}

const cargo = cargoWorkspaceVersion();
const pubspec = pubspecVersion();
const log = changelog();
if (!cargo || !pubspec || !log) {
  console.error('verify-versions: FAILED');
  process.exit();
}

console.log('cargo workspace version: ' + cargo);
console.log('pubspec version: ' + pubspec);
console.log('changelog top heading: ' + log.top);

if (pubspec !== cargo) {
  fail(
    'app/pubspec.yaml version ' + pubspec +
      ' does not match Cargo.toml version ' + cargo,
  );
}

if (!log.unreleased && log.top !== cargo) {
  fail('top CHANGELOG entry ' + log.top + ' does not match version ' + cargo);
}

// Every workflow builds with --locked, so a lockfile that still records the old
// workspace version fails the build on a version bump. Check it here, where the
// failure message can name the actual problem.
const lockfileVersions = [];
try {
  const lock = read('Cargo.lock');
  const re = /\[\[package\]\]\nname = "(batchhash-[a-z]+)"\nversion = "([^"]+)"/g;
  let match;
  while ((match = re.exec(lock)) !== null) {
    lockfileVersions.push({ name: match[1], version: match[2] });
  }
} catch {
  fail('Cargo.lock is missing');
}
const staleLock = lockfileVersions.filter((entry) => entry.version !== cargo);
if (staleLock.length > 0) {
  fail(
    'Cargo.lock records ' +
      staleLock.map((e) => e.name + '@' + e.version).join(', ') +
      ' but the workspace version is ' + cargo +
      ' — update Cargo.lock before pushing (CI builds with --locked)',
  );
}
console.log('Cargo.lock workspace crates: ' + lockfileVersions.length + ' at ' + cargo);

const tag = requestedTag();
if (tag === null) {
  console.log('no tag requested (branch push): tag consistency check skipped');
} else {
  const version = tag.replace(/^v/, '');
  console.log('requested tag: ' + tag + ' -> ' + version);
  if (version !== cargo) {
    fail('tag ' + tag + ' does not match version ' + cargo);
  }
  if (!log.released.includes(version)) {
    fail(
      'CHANGELOG.md has no released entry for ' + version + ' (found: ' +
        (log.released.join(', ') || 'none') + ')',
    );
  }
}

const toolchain = rustToolchainChannel();
const pins = workflowToolchainPins();
if (toolchain) {
  console.log('rust toolchain channel: ' + toolchain);
  for (const [channel, workflows] of pins) {
    if (channel !== toolchain) {
      fail(
        'rust-toolchain.toml pins ' + toolchain + ' but ' + workflows.join(', ') +
          ' install dtolnay/rust-toolchain@' + channel +
          ' — rustup will switch to ' + toolchain + ' inside the repository, so ' +
          'targets installed under ' + channel + ' will be missing',
      );
    }
  }
  if (pins.size === 0) {
    fail('no dtolnay/rust-toolchain pin found in any workflow');
  }

  // The channel must actually satisfy the declared MSRV floor. A channel below
  // it cannot build the locked dependency set at all: an edition-2024 crate
  // fails to parse its manifest long before any of our code compiles.
  const floor = rustVersionFloor();
  if (floor) {
    console.log('Cargo rust-version: ' + floor);
    if (compareRustVersions(toolchain, floor) < 0) {
      fail(
        'rust-toolchain.toml pins ' + toolchain + ', below the rust-version = "' +
          floor + '" floor declared in Cargo.toml',
      );
    }
  }
}

if (process.exitCode) {
  console.error('verify-versions: FAILED');
} else {
  console.log('verify-versions: OK');
}
