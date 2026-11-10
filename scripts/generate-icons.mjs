#!/usr/bin/env node
// Paints the app icon for every platform from assets/icon/BatchHash-icon.svg.
//
// Platform scaffolding is created inside CI (flutter create), so icons cannot
// be committed once for all targets. This script runs after scaffolding and
// writes the exact PNG set each platform expects, from the single SVG source.
//
// Usage: node scripts/generate-icons.mjs <app-dir> [platform|all]
//
// Exits non-zero when a requested platform directory is missing, so a scaffold
// change cannot silently ship the default Flutter icon.

import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs';
import { join } from 'node:path';
import { iconPng, iconPngSquare } from './lib/svg-icon.mjs';

const appDir = process.argv[2] ?? 'app';
const requested = process.argv[3] ?? 'all';
const svgPath = join(appDir, '..', 'assets', 'icon', 'BatchHash-icon.svg');

if (!existsSync(svgPath)) {
  console.error('generate-icons: missing ' + svgPath);
  process.exit(1);
}

const svg = readFileSync(svgPath, 'utf8');

// Two variants:
//   render      keeps the artwork's transparent rounded corners, which Android,
//               Windows and Linux composite over their own backgrounds;
//   renderFlat  is opaque and full-bleed, which the Apple icon sets require --
//               iOS and macOS draw their own mask and reject an alpha channel.
const cache = new Map();
const render = (size) => {
  const key = 'rounded:' + size;
  if (!cache.has(key)) cache.set(key, iconPng(svg, size));
  return cache.get(key);
};
const renderFlat = (size, padding = 0) => {
  const key = 'flat:' + padding + ':' + size;
  if (!cache.has(key)) cache.set(key, iconPngSquare(svg, size, { padding }));
  return cache.get(key);
};

// Android adaptive launchers show roughly the middle two thirds of the
// foreground layer, so the artwork is inset to stay inside the safe zone.
const ADAPTIVE_PADDING = 0.11;

/** Pixel dimension an AppIcon entry asks for: size x scale. */
function pixelSize(image) {
  const base = Number(String(image.size ?? '1024x1024').split('x')[0]);
  const scale = Number(String(image.scale ?? '1x').replace('x', ''));
  return Math.round(base * (Number.isFinite(scale) ? scale : 1));
}

const write = (path, size, padding = null) => {
  mkdirSync(join(path, '..'), { recursive: true });
  writeFileSync(path, padding === null ? render(size) : renderFlat(size, padding));
};

// Android launcher icons. The adaptive foreground needs padding: the launcher
// crops to a shape and shows only the middle ~66% of the layer.
const androidTargets = [
  ['mipmap-mdpi', 48, 108],
  ['mipmap-hdpi', 72, 162],
  ['mipmap-xhdpi', 96, 216],
  ['mipmap-xxhdpi', 144, 324],
  ['mipmap-xxxhdpi', 192, 432],
];

function android() {
  const res = join(appDir, 'android', 'app', 'src', 'main', 'res');
  if (!existsSync(res)) throw new Error('no android res dir at ' + res);
  for (const [dir, legacy, adaptive] of androidTargets) {
    // Pre-Android-8 launchers draw ic_launcher.png as-is.
    write(join(res, dir, 'ic_launcher.png'), legacy);
    // Adaptive icons crop the foreground layer to the launcher's shape, so the
    // artwork is painted flat (edge to edge) and inset by the generator.
    write(join(res, dir, 'ic_launcher_foreground.png'), adaptive, ADAPTIVE_PADDING);
    write(join(res, dir, 'ic_launcher_background.png'), adaptive, ADAPTIVE_PADDING);
  }
  // Adaptive icon description; the background stays the artwork's own dark
  // gradient rendered into the foreground layer, so no separate colour.
  const anydpi = join(res, 'mipmap-anydpi-v26');
  mkdirSync(anydpi, { recursive: true });
  writeFileSync(
    join(anydpi, 'ic_launcher.xml'),
    [
      '<?xml version="1.0" encoding="utf-8"?>',
      '<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">',
      '    <background android:drawable="@mipmap/ic_launcher_background" />',
      '    <foreground android:drawable="@mipmap/ic_launcher_foreground" />',
      '</adaptive-icon>',
      '',
    ].join('\n'),
  );
}

function ios() {
  const root = join(appDir, 'ios', 'Runner', 'Assets.xcassets', 'AppIcon.appiconset');
  if (!existsSync(root)) throw new Error('no iOS AppIcon.appiconset at ' + root);
  // Flutter's template ships a Contents.json with the full size list; only the
  // PNGs it names are produced here.
  const contentsPath = join(root, 'Contents.json');
  const contents = JSON.parse(readFileSync(contentsPath, 'utf8'));
  // Several entries can point at one file (iPad/iPhone share a scale), and the
  // file must hold the largest resolution any of them asks for.
  const needed = new Map();
  for (const image of contents.images ?? []) {
    if (!image.filename) continue;
    const pixels = pixelSize(image);
    needed.set(image.filename, Math.max(needed.get(image.filename) ?? 0, pixels));
  }
  // Apple icons are full-bleed: the OS applies its own rounded mask.
  for (const [filename, pixels] of needed) {
    write(join(root, filename), pixels, 0);
  }
}

function macos() {
  const root = join(appDir, 'macos', 'Runner', 'Assets.xcassets', 'AppIcon.appiconset');
  if (!existsSync(root)) throw new Error('no macOS AppIcon.appiconset at ' + root);
  const contents = JSON.parse(readFileSync(join(root, 'Contents.json'), 'utf8'));
  const needed = new Map();
  for (const image of contents.images ?? []) {
    if (!image.filename) continue;
    const pixels = pixelSize(image);
    needed.set(image.filename, Math.max(needed.get(image.filename) ?? 0, pixels));
  }
  for (const [filename, pixels] of needed) {
    write(join(root, filename), pixels, 0);
  }
}

function linux() {
  // The Flutter Linux template ships no icon at all and installs none, so the
  // PNG set is written here and a CMake install rule is appended to put it in
  // the bundle (share/icons/hicolor/<size>/apps), which is where a desktop
  // entry looks for it.
  const root = join(appDir, 'linux', 'icons');
  const sizes = [16, 24, 32, 48, 64, 128, 256, 512, 1024];
  for (const size of sizes) {
    write(join(root, size + 'x' + size, 'apps', 'batchhash.png'), size);
  }

  const cmake = join(appDir, 'linux', 'CMakeLists.txt');
  if (!existsSync(cmake)) throw new Error('no linux/CMakeLists.txt at ' + cmake);
  const marker = '# BatchHash application icons';
  const text = readFileSync(cmake, 'utf8');
  if (text.includes(marker)) return;
  const rules = [
    '',
    marker,
    '# Installed next to the bundle so desktop environments can find the icon.',
    'foreach(icon_size 16 24 32 48 64 128 256 512 1024)',
    '  install(',
    '    FILES "${CMAKE_CURRENT_SOURCE_DIR}/icons/${icon_size}x${icon_size}/apps/batchhash.png"',
    '    DESTINATION "${CMAKE_INSTALL_PREFIX}/share/icons/hicolor/${icon_size}x${icon_size}/apps"',
    '  )',
    'endforeach()',
    '',
  ].join('\n');
  writeFileSync(cmake, text.replace(/\s*$/, '\n') + rules);
}

function windows() {
  // Windows wants a multi-resolution .ico. Flutter reads
  // windows/runner/resources/app_icon.ico, so encode one here.
  const dir = join(appDir, 'windows', 'runner', 'resources');
  if (!existsSync(dir)) throw new Error('no windows resources dir at ' + dir);
  const sizes = [16, 24, 32, 48, 64, 128, 256];
  const entries = sizes.map((size) => ({ size, png: render(size) }));
  writeFileSync(join(dir, 'app_icon.ico'), encodeIco(entries));
}

/** Build a PNG-compressed .ico (supported since Windows Vista). */
function encodeIco(entries) {
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0);
  header.writeUInt16LE(1, 2); // type: icon
  header.writeUInt16LE(entries.length, 4);

  const directory = Buffer.alloc(16 * entries.length);
  let offset = header.length + directory.length;
  entries.forEach((entry, index) => {
    const base = index * 16;
    directory[base] = entry.size >= 256 ? 0 : entry.size;
    directory[base + 1] = entry.size >= 256 ? 0 : entry.size;
    directory[base + 2] = 0; // palette
    directory[base + 3] = 0; // reserved
    directory.writeUInt16LE(1, base + 4); // colour planes
    directory.writeUInt16LE(32, base + 6); // bits per pixel
    directory.writeUInt32LE(entry.png.length, base + 8);
    directory.writeUInt32LE(offset, base + 12);
    offset += entry.png.length;
  });

  return Buffer.concat([header, directory, ...entries.map((e) => e.png)]);
}

const platforms = {
  android, ios, macos, linux, windows,
};

const selected = requested === 'all' ? ['android', 'linux', 'macos', 'windows', 'ios'] : [requested];
for (const name of selected) {
  const fn = platforms[name];
  if (!fn) {
    console.error('generate-icons: unknown platform ' + name);
    process.exit(1);
  }
  const present = existsSync(join(appDir, name));
  if (!present) {
    console.error('generate-icons: ' + name + ' scaffolding missing (run flutter create first)');
    process.exit(1);
  }
  fn();
  console.log('generate-icons: painted ' + name);
}
