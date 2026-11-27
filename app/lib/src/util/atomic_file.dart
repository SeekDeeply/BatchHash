import 'dart:async';
import 'dart:io';

/// Monotonic counter so two writes in the same millisecond cannot collide on
/// the temporary file name.
int _tempCounter = 0;

/// Write [contents] to [path] so a crash leaves either the old file or the new
/// one, never a half-written one.
///
/// A plain `File.writeAsString` truncates the target first: if the process dies
/// or the disk fills between the truncate and the flush, the previous contents
/// are already gone and the file holds a prefix of the new data. For the
/// settings and log files this tool rewrites on every run, that means losing
/// configuration to a power cut.
///
/// The write goes to a temporary file in the same directory — same filesystem,
/// so the final rename is a metadata operation rather than a copy — and is
/// flushed before the rename. POSIX `rename(2)` replaces the target atomically.
///
/// Windows is the awkward case: `File.rename` refuses to replace an existing
/// file, so the target must be deleted first. That reopens a window in which
/// neither name exists, which is why the delete happens only after the
/// temporary file is complete and flushed: the worst outcome is then a missing
/// file, not a corrupt one, and a missing settings file falls back to defaults
/// while a corrupt one fails to parse.
Future<void> writeStringAtomic(String path, String contents) async {
  final File target = File(path);
  final Directory parent = target.parent;
  if (!await parent.exists()) {
    await parent.create(recursive: true);
  }
  _tempCounter += 1;
  final String tempPath =
      '$path.$pid.$_tempCounter.tmp';
  final File temp = File(tempPath);
  try {
    await temp.writeAsString(contents, flush: true);
    try {
      await temp.rename(path);
    } on FileSystemException {
      // Windows: the target exists and rename will not replace it.
      if (await target.exists()) {
        await target.delete();
      }
      await temp.rename(path);
    }
  } catch (_) {
    // A failed write must not leave debris behind: the next run would find a
    // stray `.tmp` file it cannot interpret.
    if (await temp.exists()) {
      try {
        await temp.delete();
      } on FileSystemException {
        // Nothing further to do; the original error is the one that matters.
      }
    }
    rethrow;
  }
}

/// Runs asynchronous actions one at a time, in the order they were requested.
///
/// The reason this exists rather than a bare `Future.then` chain: a chain keeps
/// the previous future, so one failing action leaves that future in an error
/// state and every later action is skipped. For a settings file that means one
/// bad write silently disables all later saves. Here each action is isolated:
/// its failure reaches its own caller and the queue keeps working.
class AsyncSerialQueue {
  Future<void> _tail = Future<void>.value();

  /// Run [action] after every previously queued action has finished.
  Future<T> run<T>(Future<T> Function() action) {
    final Completer<T> completer = Completer<T>();
    _tail = _tail.then((_) async {
      try {
        completer.complete(await action());
      } catch (error, stack) {
        completer.completeError(error, stack);
      }
    });
    // The chain link itself never carries the error, so the queue survives.
    return completer.future;
  }

  /// Completes once every action queued so far has finished.
  ///
  /// Used on teardown: a caller that is about to destroy the object must know
  /// the last write landed. Actions queued after this call are not awaited.
  Future<void> idle() => _tail;
}
