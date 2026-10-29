package com.nosdesk.plugin.share

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.ClipData
import android.content.Intent
import androidx.core.content.FileProvider
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File

@InvokeArg
class ShareFileArgs {
    lateinit var path: String
    lateinit var mimeType: String
    var title: String? = null
}

/**
 * The Android share chooser for a file the app has written to its cache.
 *
 * The file goes out as a `content://` URI from [ShareFileProvider], which only
 * exposes the cache `share/` folder, with a read grant for the app the user
 * picks. The authority matches the manifest: the applicationId (which
 * `packageName` is at runtime, debug suffix included) plus `.nosdesk.share`.
 */
@TauriPlugin
class SharePlugin(private val activity: Activity) : Plugin(activity) {
    @Command
    fun shareFile(invoke: Invoke) {
        val args = invoke.parseArgs(ShareFileArgs::class.java)
        val file = File(args.path)
        if (!file.isFile) {
            invoke.reject("file not found: ${args.path}")
            return
        }

        val authority = "${activity.packageName}.nosdesk.share"
        val uri = try {
            FileProvider.getUriForFile(activity, authority, file)
        } catch (e: IllegalArgumentException) {
            // The file is outside the folder the provider exposes.
            invoke.reject("file is not in the share folder: ${args.path}", e)
            return
        }

        val send = Intent(Intent.ACTION_SEND).apply {
            type = args.mimeType
            putExtra(Intent.EXTRA_STREAM, uri)
            args.title?.let { putExtra(Intent.EXTRA_TITLE, it) }
            // ClipData carries the grant through the chooser and gives the
            // Android 10+ sheet its preview.
            clipData = ClipData.newRawUri(args.title ?: file.name, uri)
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        val chooser = Intent.createChooser(send, args.title).apply {
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }

        activity.runOnUiThread {
            try {
                activity.startActivity(chooser)
                // Android doesn't reliably report what the user did in the
                // chooser (most targets return RESULT_CANCELED either way), so
                // a launched chooser counts as completed.
                val ret = JSObject()
                ret.put("completed", true)
                invoke.resolve(ret)
            } catch (e: ActivityNotFoundException) {
                invoke.reject("no app can receive this file", e)
            }
        }
    }
}
