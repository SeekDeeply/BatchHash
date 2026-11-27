#!/usr/bin/env node
// Adds a release signingConfig that reads android/key.properties to the
// generated Flutter Android app module. release.yml runs this only when the
// Android signing secrets exist.
//
// Usage: node scripts/patch-android-signing.mjs app/android
//
// The script fails (non-zero) when the generated project shape is not
// recognised. That is deliberate: a future Flutter template change must break
// the build instead of silently producing a debug-signed "release" artifact.

import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const androidDir = process.argv[2] ?? 'app/android';

function fail(message) {
  console.error('patch-android-signing: ' + message);
  process.exit(1);
}

const moduleDir = join(androidDir, 'app');
const candidates = [
  join(moduleDir, 'build.gradle.kts'),
  join(moduleDir, 'build.gradle'),
];
const target = candidates.find((candidate) => existsSync(candidate));
if (!target) {
  fail('no build.gradle(.kts) under ' + moduleDir);
}

const isKotlin = target.endsWith('.kts');
const source = readFileSync(target, 'utf8');

if (source.includes('keystoreProperties')) {
  console.log('patch-android-signing: ' + target + ' already reads key.properties');
  process.exit(0);
}

if (/signingConfigs\s*\{/.test(source)) {
  fail(
    'the generated template already declares signingConfigs; refusing to ' +
      'guess. Review ' + target + ' and update scripts/patch-android-signing.mjs.',
  );
}

const lines = source.split('\n');
const androidIndex = lines.findIndex((line) => /^android\s*\{\s*$/.test(line));
if (androidIndex === -1) {
  fail('no top-level "android {" block found in ' + target);
}

const loader = isKotlin
  ? [
      'val keystoreProperties = java.util.Properties()',
      'val keystorePropertiesFile = rootProject.file("key.properties")',
      'if (keystorePropertiesFile.exists()) {',
      '    keystoreProperties.load(java.io.FileInputStream(keystorePropertiesFile))',
      '}',
    ]
  : [
      'def keystoreProperties = new java.util.Properties()',
      "def keystorePropertiesFile = rootProject.file('key.properties')",
      'if (keystorePropertiesFile.exists()) {',
      '    keystoreProperties.load(new java.io.FileInputStream(keystorePropertiesFile))',
      '}',
    ];

const signingConfigs = isKotlin
  ? [
      '    signingConfigs {',
      '        create("release") {',
      '            keyAlias = keystoreProperties["keyAlias"] as String',
      '            keyPassword = keystoreProperties["keyPassword"] as String',
      '            storeFile = rootProject.file(keystoreProperties["storeFile"] as String)',
      '            storePassword = keystoreProperties["storePassword"] as String',
      '        }',
      '    }',
    ]
  : [
      '    signingConfigs {',
      '        release {',
      "            keyAlias keystoreProperties['keyAlias']",
      "            keyPassword keystoreProperties['keyPassword']",
      "            storeFile rootProject.file(keystoreProperties['storeFile'])",
      "            storePassword keystoreProperties['storePassword']",
      '        }',
      '    }',
    ];

lines.splice(androidIndex, 0, ...loader);
const androidIndexAfter = androidIndex + loader.length;
lines.splice(androidIndexAfter + 1, 0, ...signingConfigs);
let patched = lines.join('\n');

const debugPattern = isKotlin
  ? /signingConfig\s*=\s*signingConfigs\.getByName\("debug"\)/
  : /signingConfig\s+signingConfigs\.debug/;
const releaseReplacement = isKotlin
  ? 'signingConfig = signingConfigs.getByName("release")'
  : 'signingConfig signingConfigs.release';

// The replacement must hit exactly one line, inside the release build type.
// A bare `replace` matches the first occurrence anywhere: if the debug build
// type came first the release artifact stayed debug-signed while the script
// reported success, which is the worst possible outcome for a signing patch.
const matches = patched.match(new RegExp(debugPattern.source, 'g')) ?? [];
if (matches.length !== 1) {
  fail(
    'expected exactly one debug signingConfig reference in ' + target +
      ', found ' + matches.length +
      '. Review the generated template and update scripts/patch-android-signing.mjs.',
  );
}
// Locate the `release { ... }` block and confirm the reference sits inside it.
const releaseBlock = isKotlin
  ? /release\s*\{[\s\S]*?\n\s{4}\}/
  : /release\s*\{[\s\S]*?\n\s{4}\}/;
const block = patched.match(releaseBlock);
if (block === null || !debugPattern.test(block[0])) {
  fail(
    'the debug signingConfig reference is not inside the release build type in ' +
      target +
      '; refusing to patch blind.',
  );
}
patched = patched.replace(debugPattern, releaseReplacement);

writeFileSync(target, patched);
console.log('patch-android-signing: patched ' + target + ' for release signing');
