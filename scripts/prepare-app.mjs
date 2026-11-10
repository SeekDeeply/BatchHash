#!/usr/bin/env node
// One entry point for the platform scaffolding every CI job needs.
//
// Usage (from the app directory): node ../scripts/prepare-app.mjs android,linux
//
// flutter create must run before any build, because the platform directories
// are not committed. Immediately afterwards the application identifier and the
// app icons have to be applied: without them the artifacts ship the default
// com.example.<project> id and the default Flutter icon. Doing all three here
// means a new CI job cannot forget one of them.

import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

// On Windows "flutter" is a .bat shim, which spawnSync cannot execute without a
// shell: it fails with status null and no output. On POSIX a shell would add a
// quoting layer for no benefit, so it is enabled only where it is required.
const IS_WINDOWS = process.platform === 'win32';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const appDir = process.cwd();
const requested = (process.argv[2] ?? 'android,linux,macos,windows,ios').split(',')
  .map((name) => name.trim())
  .filter(Boolean);

const KNOWN = ['android', 'ios', 'linux', 'macos', 'windows'];
for (const platform of requested) {
  if (!KNOWN.includes(platform)) {
    console.error('prepare-app: unknown platform "' + platform + '"');
    process.exit(1);
  }
}

function run(command, args) {
  console.log('prepare-app: ' + command + ' ' + args.join(' '));
  const result = spawnSync(command, args, {
    stdio: 'inherit',
    cwd: appDir,
    shell: IS_WINDOWS,
  });
  if (result.error) {
    // Previously this surfaced only as "exited with null", which hides the real
    // cause (a missing executable, a .bat shim without a shell, and so on).
    console.error('prepare-app: ' + command + ' could not run: ' + result.error.message);
    process.exit(1);
  }
  if (result.status !== 0) {
    console.error(
      'prepare-app: ' + command + ' exited with ' +
        (result.status === null ? 'no status (killed by ' + result.signal + ')' : result.status),
    );
    process.exit(result.status ?? 1);
  }
}

run('flutter', [
  'create',
  '--platforms=' + requested.join(','),
  '--project-name',
  'batchhash',
  '.',
]);

// The scaffold writes a default main.dart and test only when they are absent,
// but it also rewrites pubspec fields; pub get afterwards keeps the lock honest.
run('flutter', ['pub', 'get']);

for (const platform of requested) {
  if (!existsSync(join(appDir, platform))) {
    console.error('prepare-app: flutter create did not produce ' + platform);
    process.exit(1);
  }
  run('node', [join(scriptDir, 'apply-app-id.mjs'), appDir, platform]);
  run('node', [join(scriptDir, 'generate-icons.mjs'), appDir, platform]);
}

console.log('prepare-app: ready (' + requested.join(', ') + ')');
