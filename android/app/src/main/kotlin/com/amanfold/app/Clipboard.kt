package com.amanfold.app

import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.Context
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.PersistableBundle

/**
 * Copies a value, marks it sensitive (kept out of clipboard previews and
 * history on Android 13+) and clears it after 60 seconds.
 */
object Clipboard {
    private val main = Handler(Looper.getMainLooper())
    private var clear: Runnable? = null

    fun copy(context: Context, value: String, sensitive: Boolean) {
        val cm = context.getSystemService(ClipboardManager::class.java)
        val clip = ClipData.newPlainText("Amanfold", value)
        if (sensitive) {
            clip.description.extras = PersistableBundle().apply {
                if (Build.VERSION.SDK_INT >= 33) {
                    putBoolean(ClipDescription.EXTRA_IS_SENSITIVE, true)
                } else {
                    putBoolean("android.content.extra.IS_SENSITIVE", true)
                }
            }
        }
        cm.setPrimaryClip(clip)
        clear?.let(main::removeCallbacks)
        val r = Runnable {
            runCatching { cm.clearPrimaryClip() }
        }
        clear = r
        main.postDelayed(r, 60_000)
    }
}
