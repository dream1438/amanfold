package com.amanfold.app

import android.app.Application
import android.os.Build
import android.os.Handler
import android.os.Looper
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import com.amanfold.app.security.KeystoreCipher
import com.amanfold.app.security.QuickUnlockKeys
import com.amanfold.client.Amanfold
import com.amanfold.client.ClientConfig
import com.amanfold.client.FileStore
import java.io.File

/**
 * Holds the one [Amanfold] session of this process. The app, the autofill
 * service and its screens all share it, so unlocking once unlocks all of them
 * until the auto-lock fires.
 */
class AmanfoldApp : Application() {
    lateinit var vault: Amanfold
        private set
    lateinit var prefs: Prefs
        private set
    lateinit var quickKeys: QuickUnlockKeys
        private set

    private val main = Handler(Looper.getMainLooper())
    private var backgroundSince: Long? = null
    private val lockNow = Runnable { vault.lock() }

    override fun onCreate() {
        super.onCreate()
        prefs = Prefs(this)
        quickKeys = QuickUnlockKeys(this)
        // noBackupFilesDir: never part of a backup or device transfer.
        val file = File(noBackupFilesDir, "account.bin")
        vault = Amanfold(
            store = FileStore(file, KeystoreCipher("amanfold.account")),
            config = ClientConfig(
                deviceName = "Amanfold for Android (${Build.MANUFACTURER} ${Build.MODEL})",
                allowLocalHttp = BuildConfig.ALLOW_LOCAL_HTTP,
            ),
        )
        if (!quickKeys.hasKey()) vault.disableQuickUnlock()
        ProcessLifecycleOwner.get().lifecycle.addObserver(AutoLock())
    }

    /** Locks when the app has been in the background longer than the setting. */
    private inner class AutoLock : DefaultLifecycleObserver {
        override fun onStart(owner: LifecycleOwner) {
            main.removeCallbacks(lockNow)
            val since = backgroundSince ?: return
            backgroundSince = null
            val minutes = prefs.autoLockMinutes
            if (System.currentTimeMillis() - since >= minutes * 60_000L) vault.lock()
        }

        override fun onStop(owner: LifecycleOwner) {
            backgroundSince = System.currentTimeMillis()
            val minutes = prefs.autoLockMinutes
            if (minutes == 0) vault.lock() else main.postDelayed(lockNow, minutes * 60_000L)
        }
    }
}

val android.content.Context.amanfold: AmanfoldApp get() = applicationContext as AmanfoldApp
