#!/usr/bin/env node
// Applies the release application identifier to the scaffolding that
// "flutter create" generates in CI.
//
// Usage: node scripts/apply-app-id.mjs <app-dir> <platform>
//
// Platforms: android | ios | macos | linux | windows
//
// The identifier lives in one place (scripts/app-id.mjs) so the APK, the .app
// bundle, the Windows executable metadata and the Linux desktop entry cannot
// drift apart. Missing scaffolding fails the build instead of silently
// shipping com.example.batchhash.

import { readFileSync, writeFileSync, existsSync, readdirSync, renameSync, mkdirSync } from 'node:fs';
import { join, basename } from 'node:path';
import { APP_ID, APP_ID_SEGMENTS, APP_NAME } from './app-id.mjs';

const appDir = process.argv[2] ?? 'app';
const platform = process.argv[3];

function fail(message) {
  console.error('apply-app-id: ' + message);
  process.exit(1);
}

/** Replace every match of [pattern] in [path], insisting on the match count. */
function edit(path, rules) {
  if (!existsSync(path)) fail('missing ' + path);
  let text = readFileSync(path, 'utf8');
  for (const [pattern, replacement, expected] of rules) {
    const matches = text.match(pattern) ?? [];
    if (matches.length === 0) {
      fail('no match for ' + pattern + ' in ' + path);
    }
    if (expected !== undefined && matches.length !== expected) {
      fail(
        pattern + ' matched ' + matches.length + ' times in ' + path +
          ' (expected ' + expected + '); the Flutter template changed',
      );
    }
    text = text.replace(pattern, replacement);
  }
  writeFileSync(path, text);
}

function android() {
  const moduleDir = join(appDir, 'android', 'app');
  const gradle = ['build.gradle.kts', 'build.gradle']
    .map((name) => join(moduleDir, name))
    .find((candidate) => existsSync(candidate));
  if (!gradle) fail('no build.gradle(.kts) in ' + moduleDir);

  // The template declares both namespace and applicationId.
  edit(gradle, [
    [/namespace\s*=\s*"[^"]*"/g, 'namespace = "' + APP_ID + '"', 1],
    [/applicationId\s*=\s*"[^"]*"/g, 'applicationId = "' + APP_ID + '"', 1],
  ]);
  if (readFileSync(gradle, 'utf8').includes('com.example.')) {
    fail('com.example still present in ' + gradle);
  }

  // The Kotlin/Java entry point declares its package and must move with it.
  const kotlinRoot = join(moduleDir, 'src', 'main', 'kotlin');
  const javaRoot = join(moduleDir, 'src', 'main', 'java');
  const root = existsSync(kotlinRoot) ? kotlinRoot : javaRoot;
  if (existsSync(root)) {
    const target = join(root, ...APP_ID_SEGMENTS);
    const walk = (dir) => {
      for (const entry of readdirSync(dir, { withFileTypes: true })) {
        const full = join(dir, entry.name);
        if (entry.isDirectory()) {
          if (full !== target) walk(full);
          continue;
        }
        if (!entry.name.endsWith('.kt') && !entry.name.endsWith('.java')) continue;
        const body = readFileSync(full, 'utf8');
        if (!/^package\s+com\.example\./m.test(body)) continue;
        mkdirSync(target, { recursive: true });
        writeFileSync(
          full,
          body.replace(/^package\s+com\.example\.[\w.]*/m, 'package ' + APP_ID),
        );
        renameSync(full, join(target, basename(full)));
      }
    };
    walk(root);
  }

  const manifest = join(moduleDir, 'src', 'main', 'AndroidManifest.xml');
  if (existsSync(manifest)) {
    const body = readFileSync(manifest, 'utf8');
    if (body.includes('com.example.')) {
      writeFileSync(manifest, body.replace(/com\.example\.[\w.]*/g, APP_ID));
    }
  }

  const label = join(moduleDir, 'src', 'main', 'AndroidManifest.xml');
  if (existsSync(label)) {
    const body = readFileSync(label, 'utf8');
    const updated = body.replace(/android:label="[^"]*"/, 'android:label="' + APP_NAME + '"');
    if (updated !== body) writeFileSync(label, updated);
  }
}

/**
 * Set an Info.plist string value, creating the key when it is absent.
 * The Flutter templates ship CFBundleDisplayName = {{projectName}}.
 */
function setPlistString(plistPath, key, value) {
  if (!existsSync(plistPath)) return false;
  const text = readFileSync(plistPath, 'utf8');
  const pattern = new RegExp(
    '(<key>' + key + '</key>\\s*<string>)[^<]*(</string>)',
  );
  if (!pattern.test(text)) {
    fail('no ' + key + ' entry in ' + plistPath);
  }
  writeFileSync(plistPath, text.replace(pattern, '$1' + value + '$2'));
  return true;
}

/**
 * iOS keeps the bundle id in the Xcode project, one entry per configuration,
 * plus one per configuration for RunnerTests.
 */
function ios() {
  const project = join(appDir, 'ios', 'Runner.xcodeproj', 'project.pbxproj');
  if (!existsSync(project)) fail('missing ' + project);
  let text = readFileSync(project, 'utf8');

  text = text.replace(
    /PRODUCT_BUNDLE_IDENTIFIER = [^;]*\.RunnerTests;/g,
    'PRODUCT_BUNDLE_IDENTIFIER = ' + APP_ID + '.RunnerTests;',
  );
  const appMatches = text.match(/PRODUCT_BUNDLE_IDENTIFIER = (?![^;]*RunnerTests)[^;]+;/g) ?? [];
  if (appMatches.length === 0) fail('no PRODUCT_BUNDLE_IDENTIFIER in ' + project);
  text = text.replace(
    /PRODUCT_BUNDLE_IDENTIFIER = (?![^;]*RunnerTests)[^;]+;/g,
    'PRODUCT_BUNDLE_IDENTIFIER = ' + APP_ID + ';',
  );
  writeFileSync(project, text);

  const remaining = readFileSync(project, 'utf8').match(/com\.example\.[\w.]*/g) ?? [];
  if (remaining.length > 0) {
    fail('com.example still present in ' + project + ': ' + remaining.join(', '));
  }
  setPlistString(join(appDir, 'ios', 'Runner', 'Info.plist'), 'CFBundleDisplayName', APP_NAME);
  console.log('apply-app-id: ' + appMatches.length + ' iOS build configurations');
}

/**
 * macOS is different: Runner.xcodeproj only carries the RunnerTests bundle id.
 * The app's own identifier lives in Runner/Configs/AppInfo.xcconfig, which the
 * target build configurations include via baseConfigurationReference.
 */
function macos() {
  const appInfo = join(appDir, 'macos', 'Runner', 'Configs', 'AppInfo.xcconfig');
  if (!existsSync(appInfo)) fail('missing ' + appInfo);
  const body = readFileSync(appInfo, 'utf8');
  if (!/^PRODUCT_BUNDLE_IDENTIFIER\s*=/m.test(body)) {
    fail('no PRODUCT_BUNDLE_IDENTIFIER in ' + appInfo);
  }
  let updated = body.replace(
    /^PRODUCT_BUNDLE_IDENTIFIER\s*=.*$/m,
    'PRODUCT_BUNDLE_IDENTIFIER = ' + APP_ID,
  );
  // PRODUCT_COPYRIGHT is where the template's {{organization}} lands, so
  // leaving it alone ships "Copyright © 2026 com.example" in the bundle.
  updated = updated
    .replace(/^PRODUCT_COPYRIGHT\s*=.*$/m, 'PRODUCT_COPYRIGHT = Copyright © 2026 SeekDeeply. All rights reserved.')
    .replace(/^PRODUCT_NAME\s*=.*$/m, 'PRODUCT_NAME = ' + APP_NAME);
  writeFileSync(appInfo, updated);

  const project = join(appDir, 'macos', 'Runner.xcodeproj', 'project.pbxproj');
  if (existsSync(project)) {
    const text = readFileSync(project, 'utf8').replace(
      /PRODUCT_BUNDLE_IDENTIFIER = [^;]*\.RunnerTests;/g,
      'PRODUCT_BUNDLE_IDENTIFIER = ' + APP_ID + '.RunnerTests;',
    );
    writeFileSync(project, text);
  }
  setPlistString(join(appDir, 'macos', 'Runner', 'Info.plist'), 'CFBundleName', APP_NAME);
  console.log('apply-app-id: macOS AppInfo.xcconfig updated');
}

function linux() {
  // Linux identifies the app through the GTK application id.
  // The GTK application id is a CMake variable (APPLICATION_ID) that
  // my_application.cc consumes, so the value must be set where it is defined.
  const cmake = join(appDir, 'linux', 'CMakeLists.txt');
  if (!existsSync(cmake)) fail('missing ' + cmake);
  const body = readFileSync(cmake, 'utf8');
  const updated = body.replace(
    /set\(APPLICATION_ID\s+"[^"]*"\)/,
    'set(APPLICATION_ID "' + APP_ID + '")',
  );
  if (updated === body) fail('no APPLICATION_ID entry in ' + cmake);
  writeFileSync(cmake, updated);

  // The window/app title in my_application.cc is what the desktop shell shows.
  const cc = join(appDir, 'linux', 'runner', 'my_application.cc');
  if (existsSync(cc)) {
    const source = readFileSync(cc, 'utf8');
    const retitled = source
      .replace(/(gtk_window_set_title\s*\(\s*window\s*,\s*")[^"]*(")/g, '$1' + APP_NAME + '$2')
      .replace(/(gtk_header_bar_set_title\s*\([^,]+,\s*")[^"]*(")/g, '$1' + APP_NAME + '$2');
    if (retitled !== source) writeFileSync(cc, retitled);
  }
}

function windows() {
  // Windows has no bundle identifier; the stable identity users see is the
  // executable metadata in Runner.rc.
  const rc = join(appDir, 'windows', 'runner', 'Runner.rc');
  if (!existsSync(rc)) fail('missing ' + rc);
  let text = readFileSync(rc, 'utf8');
  // ProductName / FileDescription are what Explorer and Task Manager show.
  const before = text;
  text = text.replace(/(VALUE "CompanyName",\s*")[^"]*(")/g, '$1SeekDeeply$2');
  text = text.replace(/(VALUE "ProductName",\s*")[^"]*(")/g, '$1' + APP_NAME + '$2');
  text = text.replace(/(VALUE "FileDescription",\s*")[^"]*(")/g, '$1' + APP_NAME + '$2');
  text = text.replace(/(VALUE "InternalName",\s*")[^"]*(")/g, '$1batchhash$2');
  text = text.replace(/(VALUE "OriginalFilename",\s*")[^"]*(")/g, '$1batchhash.exe$2');
  text = text.replace(
    /(VALUE "LegalCopyright",\s*")[^"]*(")/g,
    '$1Copyright (C) 2026 SeekDeeply$2',
  );
  if (text === before) fail('no VERSIONINFO strings in ' + rc);
  writeFileSync(rc, text);
}

const handlers = { android, ios, macos, linux, windows };
const handler = handlers[platform];
if (!handler) fail('unknown platform "' + platform + '" (use android|ios|macos|linux|windows)');
handler();
console.log('apply-app-id: ' + platform + ' -> ' + APP_ID);
