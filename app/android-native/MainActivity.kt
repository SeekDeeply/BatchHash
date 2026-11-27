package io.github.seekdeeply.batchhash

import android.content.ActivityNotFoundException
import android.content.Intent
import android.os.Handler
import android.os.Looper
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Executors
import android.os.ParcelFileDescriptor
import android.net.Uri
import android.provider.DocumentsContract
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel

/**
 * Storage Access Framework bridge.
 *
 * Android 11+ hands the folder picker back a content:// tree URI, and dart:io
 * cannot list or read through one: File only understands POSIX paths. That is
 * an upstream limitation rather than a bug in this app (see
 * flutter_file_picker#1552 and dart-lang/sdk#54878), so the only way to make
 * "pick a folder" work is to walk the tree through the platform.
 *
 * This channel answers two questions and nothing else: which folder did the
 * user grant, and what is inside it. Reading file bytes is deliberately not
 * here; the Dart side opens the descriptors it needs and hands them to the
 * Rust core, so no hashing happens on a thread this class controls.
 */
class MainActivity : FlutterActivity() {
    private val channelName = "batchhash/saf"
    private val pickTreeRequest = 0x5A11

    private var pendingResult: MethodChannel.Result? = null

    /**
     * Descriptors opened for [openFiles], held so their finalizers cannot
     * close the underlying fd while the Rust core is still reading it.
     *
     * A raw fd number is only meaningful while something keeps it open; once
     * the ParcelFileDescriptor is collected the OS closes the descriptor and
     * the number becomes a different file, or nothing. Keyed by relative path
     * so [closeFiles] can release exactly what was opened.
     */
    private val openDescriptors = ConcurrentHashMap<String, ParcelFileDescriptor>()

    /**
     * Worker for every ContentResolver call.
     *
     * A tree walk on a large folder or a slow provider is seconds of blocking
     * `query`/`openFileDescriptor` calls. Run on the platform main thread they
     * freeze the UI and, worse, the method-channel reply arrives late enough
     * that Flutter has already moved on.
     */
    /** Posts a worker result back to the platform main thread. */
    private val mainHandler = Handler(Looper.getMainLooper())

    private val ioExecutor = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "batchhash-saf-io").apply { isDaemon = true }
    }

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, channelName)
            .setMethodCallHandler { call, result ->
                when (call.method) {
                    // Resolves with the granted tree URI, or null when the user
                    // backed out. A refused pick and a cancelled pick look the
                    // same from Dart unless the distinction is made here, and
                    // it is not: both are "no folder".
                    "pickDirectory" -> {
                        pendingResult?.error(
                            "ALREADY_PICKING",
                            "a folder picker is already open",
                            null,
                        )
                        pendingResult = result
                        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).apply {
                            addFlags(
                                Intent.FLAG_GRANT_READ_URI_PERMISSION or
                                    Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION,
                            )
                        }
                        try {
                            startActivityForResult(intent, pickTreeRequest)
                        } catch (error: ActivityNotFoundException) {
                            // A stripped ROM, a work profile or an Android TV build
                            // can ship without DocumentsUI. The activity is never
                            // started, so onActivityResult never runs and the result
                            // would stay pending forever, blocking every later pick.
                            pendingResult = null
                            result.error(
                                "NO_PICKER",
                                "this device has no folder picker",
                                null,
                            )
                        }
                    }

                    // Walks the granted tree and returns every node as a map of
                    // path / is_dir / size / mtime, with paths relative to the
                    // tree root so the caller can key on them.
                    "listTree" -> {
                        val uri = call.argument<String>("uri")
                        val maxDepth = call.argument<Int>("maxDepth") ?: 8
                        if (uri.isNullOrEmpty()) {
                            result.error("BAD_ARGS", "uri is required", null)
                            return@setMethodCallHandler
                        }
                        // Off the main thread: a deep tree is hundreds of
                        // provider queries, each of which can take milliseconds.
                        ioExecutor.execute {
                            try {
                                val nodes = collectTree(Uri.parse(uri), maxDepth)
                                mainHandler.post { result.success(nodes) }
                            } catch (error: SecurityException) {
                                // The grant was revoked after the picker returned.
                                mainHandler.post {
                                    result.error(
                                        "PERMISSION_REVOKED",
                                        "the folder permission was withdrawn",
                                        null,
                                    )
                                }
                            } catch (error: Exception) {
                                mainHandler.post {
                                    result.error("LIST_FAILED", error.message, null)
                                }
                            }
                        }
                    }

                    // Opens one descriptor per requested path and returns the
                    // raw fd numbers together with the authoritative size.
                    //
                    // The size comes from the descriptor rather than from the
                    // tree listing, which can be stale: a file changed between
                    // the two calls would otherwise be hashed against a length
                    // that no longer matches its contents.
                    "openFiles" -> {
                        val uri = call.argument<String>("uri")
                        val documents =
                            call.argument<List<Map<String, String>>>("documents")
                        if (uri.isNullOrEmpty() || documents == null) {
                            result.error(
                                "BAD_ARGS",
                                "uri and documents are required",
                                null,
                            )
                            return@setMethodCallHandler
                        }
                        ioExecutor.execute {
                            try {
                                val opened = openFiles(Uri.parse(uri), documents)
                                mainHandler.post { result.success(opened) }
                            } catch (error: SecurityException) {
                                mainHandler.post {
                                    result.error(
                                        "PERMISSION_REVOKED",
                                        "the folder permission was withdrawn",
                                        null,
                                    )
                                }
                            } catch (error: Exception) {
                                mainHandler.post {
                                    result.error("OPEN_FAILED", error.message, null)
                                }
                            }
                        }
                    }

                    // Releases what [openFiles] opened. Safe to call twice: a
                    // path that is not open is skipped rather than reported.
                    "closeFiles" -> {
                        val handles = call.argument<List<String>>("handles")
                        if (handles != null) {
                            // Queued on the same single-thread executor as
                            // openFiles: closing on the main thread could run
                            // before an open that is still queued, and then the
                            // descriptor would be opened after it was closed.
                            ioExecutor.execute {
                                closeFiles(handles)
                                mainHandler.post { result.success(null) }
                            }
                        } else {
                            result.success(null)
                        }
                    }

                    else -> result.notImplemented()
                }
            }
    }

    override fun onDestroy() {
        // The activity can be destroyed with descriptors still open: the
        // Flutter side may be gone before it calls closeFiles. Leaving them to
        // the finalizer leaks real file handles on a device that has few.
        for (key in openDescriptors.keys.toList()) {
            openDescriptors.remove(key)?.let { descriptor ->
                try {
                    descriptor.close()
                } catch (_: Exception) {
                    // Nothing left to do while tearing down.
                }
            }
        }
        ioExecutor.shutdownNow()
        super.onDestroy()
    }

    override fun onActivityResult(
        requestCode: Int,
        resultCode: Int,
        resultData: Intent?,
    ) {
        super.onActivityResult(requestCode, resultCode, resultData)
        if (requestCode != pickTreeRequest) {
            return
        }
        val pending = pendingResult
        pendingResult = null
        if (pending == null) {
            return
        }
        if (resultCode != RESULT_OK || resultData == null) {
            pending.success(null)
            return
        }
        val uri = resultData.data
        if (uri == null) {
            pending.success(null)
            return
        }
        // Persist the grant so a later scan does not have to ask again; without
        // this the permission dies with the activity.
        try {
            contentResolver.takePersistableUriPermission(
                uri,
                Intent.FLAG_GRANT_READ_URI_PERMISSION,
            )
        } catch (_: SecurityException) {
            // The provider refused to persist. The pick is still usable for
            // this session, so continue rather than failing the whole flow.
        }
        pending.success(uri.toString())
    }

    /**
     * Every node under [treeUri], depth-first, at most [maxDepth] levels of
     * directories below the root.
     *
     * Recursion happens here rather than in Dart because each level would
     * otherwise be a channel round trip, and a deep tree costs hundreds of
     * them for no benefit.
     */
    private fun collectTree(treeUri: Uri, maxDepth: Int): List<Map<String, Any?>> {
        val nodes = ArrayList<Map<String, Any?>>()
        val rootDocumentId = DocumentsContract.getTreeDocumentId(treeUri)
        walk(treeUri, rootDocumentId, "", 0, maxDepth, nodes)
        return nodes
    }

    private fun walk(
        treeUri: Uri,
        parentDocumentId: String,
        prefix: String,
        depth: Int,
        maxDepth: Int,
        out: MutableList<Map<String, Any?>>,
    ) {
        // Address the subtree through the parent document id. Appending a path
        // to the tree URI instead produces a URI no provider accepts, which is
        // a subtle way to get an empty listing that looks like an empty
        // folder.
        val childrenUri = DocumentsContract.buildChildDocumentsUriUsingTree(
            treeUri,
            parentDocumentId,
        )
        val projection = arrayOf(
            DocumentsContract.Document.COLUMN_DOCUMENT_ID,
            DocumentsContract.Document.COLUMN_DISPLAY_NAME,
            DocumentsContract.Document.COLUMN_SIZE,
            DocumentsContract.Document.COLUMN_LAST_MODIFIED,
            DocumentsContract.Document.COLUMN_MIME_TYPE,
        )
        contentResolver.query(childrenUri, projection, null, null, null)?.use { cursor ->
            val idIndex = cursor.getColumnIndex(DocumentsContract.Document.COLUMN_DOCUMENT_ID)
            val nameIndex =
                cursor.getColumnIndex(DocumentsContract.Document.COLUMN_DISPLAY_NAME)
            val sizeIndex = cursor.getColumnIndex(DocumentsContract.Document.COLUMN_SIZE)
            val mtimeIndex =
                cursor.getColumnIndex(DocumentsContract.Document.COLUMN_LAST_MODIFIED)
            val mimeIndex = cursor.getColumnIndex(DocumentsContract.Document.COLUMN_MIME_TYPE)
            while (cursor.moveToNext()) {
                val documentId = if (idIndex >= 0) cursor.getString(idIndex) else null
                val name = if (nameIndex >= 0) cursor.getString(nameIndex) else null
                if (documentId.isNullOrEmpty() || name.isNullOrEmpty()) {
                    continue
                }
                // A provider may report no size for a directory; zero is the
                // only honest answer available.
                // A provider is allowed to report no size. Reporting 0 for
                // "unknown" made every such file look empty, and a hash report
                // that claims a file is zero bytes is worse than one that says
                // the size is unknown.
                val size: Long? =
                    if (sizeIndex >= 0 && !cursor.isNull(sizeIndex)) {
                        cursor.getLong(sizeIndex).takeIf { it >= 0L }
                    } else {
                        null
                    }
                val mtime =
                    if (mtimeIndex >= 0 && !cursor.isNull(mtimeIndex)) {
                        cursor.getLong(mtimeIndex)
                    } else {
                        0L
                    }
                val mime = if (mimeIndex >= 0) cursor.getString(mimeIndex) else null
                val isDirectory = mime == DocumentsContract.Document.MIME_TYPE_DIR
                val relative = if (prefix.isEmpty()) name else prefix + "/" + name
                out.add(
                    mapOf<String, Any?>(
                        "path" to relative,
                        // The provider document id is the only address that works
                        // for a later open: rebuilding it by joining the root id
                        // with a relative path is a guess that fails on any
                        // provider whose ids are not path shaped.
                        "document_id" to documentId,
                        "is_dir" to isDirectory,
                        "size" to size,
                        // Milliseconds since the epoch, matching what the Dart
                        // side expects from a filesystem stat.
                        "mtime_ms" to mtime,
                    ),
                )
                if (isDirectory && depth < maxDepth) {
                    walk(treeUri, documentId, relative, depth + 1, maxDepth, out)
                }
            }
        }
    }

    /**
     * Open a read-only descriptor for each requested document.
     *
     * [documents] carries the provider document id for each path, taken from the
     * tree listing. The id is used directly: rebuilding it as
     * `rootDocumentId + "/" + path` only works for providers whose ids happen to
     * look like paths, and fails outright on the ones that use opaque ids.
     *
     * Returns the fd, the handle that identifies it for [closeFiles], and the
     * descriptor's own size. Paths that cannot be opened are omitted so the
     * caller hashes what it could rather than failing the whole batch.
     */
    private fun openFiles(
        treeUri: Uri,
        documents: List<Map<String, String>>,
    ): List<Map<String, Any?>> {
        val opened = ArrayList<Map<String, Any?>>(documents.size)
        for (entry in documents) {
            val path = entry["path"] ?: continue
            val documentId = entry["document_id"] ?: continue
            val documentUri = try {
                DocumentsContract.buildDocumentUriUsingTree(treeUri, documentId)
            } catch (_: Exception) {
                continue
            }
            val descriptor = try {
                contentResolver.openFileDescriptor(documentUri, "r")
            } catch (_: SecurityException) {
                // The grant can be revoked between listing and opening. Skipping
                // the file keeps the rest of the batch usable; the caller reports
                // the path as unopened.
                null
            } catch (_: Exception) {
                null
            } ?: continue
            // A handle unique per open, not the path: the same path can be opened
            // twice (a retry, two pages), and keying by path made the second open
            // overwrite the first entry, so closing it left the first descriptor
            // open with no reference left to close it.
            val handle = "h" + descriptor.fd + "-" + System.nanoTime()
            openDescriptors.put(handle, descriptor)?.let { previous ->
                try {
                    previous.close()
                } catch (_: Exception) {
                    // Replacing a stale entry; the new one is what matters.
                }
            }
            opened.add(
                mapOf<String, Any?>(
                    "path" to path,
                    "handle" to handle,
                    "fd" to descriptor.fd,
                    // `statSize` is -1 when the provider cannot report a size;
                    // passing that through as a huge unsigned value would make the
                    // file look enormous and the progress bar meaningless.
                    "size" to descriptor.statSize.takeIf { it >= 0L },
                ),
            )
        }
        return opened
    }
    private fun closeFiles(handles: List<String>) {
        for (handle in handles) {
            val descriptor = openDescriptors.remove(handle) ?: continue
            try {
                descriptor.close()
            } catch (_: Exception) {
                // Closing an already-closed descriptor is not worth failing a
                // hash run over.
            }
        }
    }
}
