package com.nosdesk.plugin.securestore

import android.app.Activity
import android.content.SharedPreferences
import androidx.security.crypto.EncryptedSharedPreferences
import androidx.security.crypto.MasterKey
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

@InvokeArg
class SaveArgs {
    var token: String? = null
}

/**
 * Android Keystore-backed secure storage for the auth refresh token.
 *
 * `EncryptedSharedPreferences` encrypts both keys and values with a master key
 * held in the Android Keystore (hardware-backed where the device supports it),
 * so the token is not readable from the app's data dir at rest. The plugin gets
 * the `Activity` (a valid `Context`) by constructor injection from the Tauri
 * plugin framework. Writes use `commit()` so the value is durable even if the
 * process is killed right after.
 */
@TauriPlugin
class SecureStorePlugin(private val activity: Activity) : Plugin(activity) {
    private val fileName = "nosdesk_secure_store"
    private val key = "refresh_token"

    private fun prefs(): SharedPreferences {
        val app = activity.applicationContext
        val masterKey = MasterKey.Builder(app)
            .setKeyScheme(MasterKey.KeyScheme.AES256_GCM)
            .build()
        return EncryptedSharedPreferences.create(
            app,
            fileName,
            masterKey,
            EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
            EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM,
        )
    }

    @Command
    fun save(invoke: Invoke) {
        val args = invoke.parseArgs(SaveArgs::class.java)
        val token = args.token
        if (token == null) {
            invoke.reject("token is required")
            return
        }
        try {
            prefs().edit().putString(key, token).commit()
            invoke.resolve()
        } catch (e: Exception) {
            invoke.reject("secure store save failed: ${e.message}", e)
        }
    }

    @Command
    fun load(invoke: Invoke) {
        // Never reject on a read failure: the caller restores the session at
        // startup, and a thrown error there would block login. Absent / unreadable
        // both mean "no usable session" -> null.
        val value = try {
            prefs().getString(key, null)
        } catch (e: Exception) {
            null
        }
        val ret = JSObject()
        ret.put("value", value)
        invoke.resolve(ret)
    }

    @Command
    fun clear(invoke: Invoke) {
        try {
            prefs().edit().remove(key).commit()
        } catch (e: Exception) {
            // Best effort: a failed clear must not surface as a hard error.
        }
        invoke.resolve()
    }
}
