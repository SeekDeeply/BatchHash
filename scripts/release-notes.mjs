#!/usr/bin/env node
// Builds the GitHub Release body from CHANGELOG.md.
//
// generate_release_notes alone produces "What's Changed" from commit subjects,
// which for a curated project says nothing useful -- a 1.0 release body that
// only mentions a README typo is worse than no body at all. The CHANGELOG
// already carries the user-facing description, so it is the source here.
//
// Usage: node scripts/release-notes.mjs <tag> [changelog] > notes.md

import { readFileSync } from 'node:fs';

const tag = process.argv[2];
if (!tag) {
  console.error('release-notes: a tag (or version) is required');
  process.exit(1);
}
const version = tag.replace(/^v/, '');
const changelogPath = process.argv[3] ?? 'CHANGELOG.md';

let text;
try {
  text = readFileSync(changelogPath, 'utf8');
} catch {
  console.error('release-notes: cannot read ' + changelogPath);
  process.exit(1);
}

// Sections look like "## [x.y.z] - date" and run until the next "## [".
const lines = text.split('\n');
let start = -1;
let end = lines.length;
for (let i = 0; i < lines.length; i += 1) {
  const heading = /^##\s+\[([^\]]+)\](.*)$/.exec(lines[i]);
  if (!heading) continue;
  if (start !== -1) {
    end = i;
    break;
  }
  if (heading[1].trim() === version) start = i;
}

if (start === -1) {
  console.error('release-notes: CHANGELOG.md has no section for ' + version);
  process.exit(1);
}

const body = lines.slice(start + 1, end).join('\n').trim();

if (body.length === 0) {
  console.error('release-notes: the ' + version + ' CHANGELOG section is empty');
  process.exit(1);
}

// Demote the section's own headings so they sit under the release title.
const demoted = body.replace(/^###\s+/gm, '#### ');

const assets = [
  '## 下载',
  '',
  '| 平台 | 文件 |',
  '|---|---|',
  '| Android | app-release.aab（上架用）、app-arm64-v8a-release.apk、app-armeabi-v7a-release.apk、app-x86_64-release.apk |',
  '| Windows | BatchHash-windows.zip（解压后运行 batchhash.exe） |',
  '| macOS | BatchHash-macos.zip |',
  '| Linux | BatchHash-linux.tar.gz |',
  '',
  '应用标识：io.github.seekdeeply.batchhash。',
  '',
  '> 二进制未做代码签名：Windows SmartScreen 与 macOS Gatekeeper 会提示；',
  '> Android 若未配置签名密钥则为 debug 签名，不可直接上架。',
];

console.log(demoted + '\n\n' + assets.join('\n'));
