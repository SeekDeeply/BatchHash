package io.github.seekdeeply.batchhash

import android.content.Intent
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
    private val openDescriptors = HashMap<String, ParcelFileDescriptor>()

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
                        startActivityForResult(intent, pickTreeRequest)
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
                        try {
                            result.success(collectTree(Uri.parse(uri), maxDepth))
                        } catch (error: Exception) {
                            result.error("LIST_FAILED", error.message, null)
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
                        val paths = call.argument<List<String>>("paths")
                        if (uri.isNullOrEmpty() || paths == null) {
                            result.error(
                                "BAD_ARGS",
                                "uri and paths are required",
                                null,
                            )
                            return@setMethodCallHandler
                        }
                        try {
                            result.success(openFiles(Uri.parse(uri), paths))
                        } catch (error: Exception) {
                            result.error("OPEN_FAILED", error.message, null)
                        }
                    }

                    // Releases what [openFiles] opened. Safe to call twice: a
                    // path that is not open is skipped rather than reported.
                    "closeFiles" -> {
                        val paths = call.argument<List<String>>("paths")
                        if (paths != null) {
                            closeFiles(paths)
                        }
                        result.success(null)
                    }

                    else -> result.notImplemented()
                }
            }
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
    private fun collectTree(treeUri: Uri, maxDepth: Int): List<Map<String, Any>> {
        val nodes = ArrayList<Map<String, Any>>()
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
        out: MutableList<Map<String, Any>>,
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
                val size = if (sizeIndex >= 0) cursor.getLong(sizeIndex) else 0L
                val mtime = if (mtimeIndex >= 0) cursor.getLong(mtimeIndex) else 0L
                val mime = if (mimeIndex >= 0) cursor.getString(mimeIndex) else null
                val isDirectory = mime == DocumentsContract.Document.MIME_TYPE_DIR
                val relative = if (prefix.isEmpty()) name else prefix + "/" + name
                out.add(
                    mapOf(
                        "path" to relative,
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
     * Open a read-only descriptor for each of [paths] inside [treeUri].
     *
     * Returns the raw fd number and the descriptor's own size for every path
     * that could be opened. Paths that cannot be opened are omitted, so the
     * caller hashes what it could and says so rather than failing the batch.
     */
    private fun openFiles(
        treeUri: Uri,
        paths: List<String>,
    ): List<Map<String, Any>> {
        val rootDocumentId = DocumentsContract.getTreeDocumentId(treeUri)
        val opened = ArrayList<Map<String, Any>>(paths.size)
        for (path in paths) {
            // Address each file through its own document id, which is the tree
            // document id plus the relative path. A path appended straight onto
            // the tree URI is rejected by every provider.
            val documentUri = DocumentsContract.buildDocumentUriUsingTree(
                treeUri,
                rootDocumentId + "/" + path,
            )
            val descriptor = try {
                contentResolver.openFileDescriptor(documentUri, "r")
            } catch (_: Exception) {
                null
            } ?: continue
            openDescriptors[path] = descriptor
            opened.add(
                mapOf(
                    "path" to path,
                    "fd" to descriptor.fd,
                    "size" to descriptor.statSize,
                ),
            )
        }
        return opened
    }

    private fun closeFiles(paths: List<String>) {
        for (path in paths) {
            val descriptor = openDescriptors.remove(path) ?: continue
            try {
                descriptor.close()
            } catch (_: Exception) {
                // Closing an already-closed descriptor is not worth failing a
                // hash run over.
            }
        }
    }
}
