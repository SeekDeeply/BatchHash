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

if (!debugPattern.test(patched)) {
  fail('could not find the debug signingConfig reference to replace in ' + target);
}
patched = patched.replace(debugPattern, releaseReplacement);

writeFileSync(target, patched);
console.log('patch-android-signing: patched ' + target + ' for release signing');
