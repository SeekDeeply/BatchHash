#!/usr/bin/env node
// Version / tag consistency gate for CI and the release workflow.
//
// Reads the workspace Cargo version, the Flutter app version, the top
// CHANGELOG heading and (when a tag is being built) the pushed tag, then exits
// non-zero on any mismatch. No dependencies beyond Node's standard library.
//
// Usage:
//   node scripts/verify-versions.mjs
//   node scripts/verify-versions.mjs --tag v0.1.0-rc.1
//
// In GitHub Actions the tag is read from GITHUB_REF / GITHUB_REF_NAME. A branch
// push has no tag, so only cargo/pubspec/changelog consistency is checked: the
// repository ships 0.1.0-rc.1 while the CHANGELOG top section is [未发布], which
// must stay a passing state.

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

if (process.exitCode) {
  console.error('verify-versions: FAILED');
} else {
  console.log('verify-versions: OK');
}
