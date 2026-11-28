package com.amanfold.app

import android.content.Context

/** Ordinary settings. Nothing secret lives here. */
class Prefs(context: Context) {
    private val p = context.getSharedPreferences("settings", Context.MODE_PRIVATE)

    var server: String
        get() = p.getString("server", null) ?: BuildConfig.DEFAULT_SERVER
        set(v) = p.edit().putString("server", v).apply()

    /** 0 locks as soon as the app leaves the screen. */
    var autoLockMinutes: Int
        get() = p.getInt("autoLock", 5).takeIf { it in AUTO_LOCK_CHOICES } ?: 5
        set(v) = p.edit().putInt("autoLock", v).apply()

    /** Whether we already offered quick unlock after sign-in. */
    var quickUnlockOffered: Boolean
        get() = p.getBoolean("quickOffered", false)
        set(v) = p.edit().putBoolean("quickOffered", v).apply()

    fun clearAccountState() {
        p.edit().remove("quickOffered").apply()
    }

    companion object {
        val AUTO_LOCK_CHOICES = listOf(0, 1, 5, 15, 60)
    }
}
