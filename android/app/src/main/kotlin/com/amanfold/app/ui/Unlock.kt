package com.amanfold.app.ui

import androidx.activity.compose.LocalActivity
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Fingerprint
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.fragment.app.FragmentActivity
import com.amanfold.app.R
import com.amanfold.app.amanfold
import com.amanfold.app.security.authenticate
import kotlinx.coroutines.launch

enum class QuickResult { Unlocked, Cancelled, Changed }

/** Unlock with the biometric-protected device key. */
suspend fun quickUnlock(activity: FragmentActivity): QuickResult {
    val app = activity.amanfold
    val cipher = app.quickKeys.decryptCipher()
    if (cipher == null) {
        app.vault.disableQuickUnlock()
        return QuickResult.Changed
    }
    val ok = activity.authenticate(
        activity.getString(R.string.biometric_title),
        activity.getString(R.string.use_password),
        cipher,
    ) ?: return QuickResult.Cancelled
    val deviceKey = app.quickKeys.open(ok)
    try {
        app.vault.quickUnlock(deviceKey)
    } catch (e: Throwable) {
        app.quickKeys.clear()
        app.vault.disableQuickUnlock()
        return QuickResult.Changed
    } finally {
        deviceKey.fill(0)
    }
    return QuickResult.Unlocked
}

/** Turn quick unlock on: needs one biometric check to seal the device key. */
suspend fun enableQuickUnlock(activity: FragmentActivity): Boolean {
    val app = activity.amanfold
    val cipher = app.quickKeys.encryptCipher()
    val ok = activity.authenticate(
        activity.getString(R.string.biometric_enable_title),
        activity.getString(R.string.cancel),
        cipher,
    ) ?: run {
        app.quickKeys.clear()
        return false
    }
    val setup = app.vault.enableQuickUnlock()
    app.quickKeys.save(ok, setup.deviceKey)
    return true
}

fun disableQuickUnlock(activity: FragmentActivity) {
    activity.amanfold.quickKeys.clear()
    activity.amanfold.vault.disableQuickUnlock()
}

@Composable
fun UnlockScreen(onSignOut: (() -> Unit)?) {
    val activity = LocalActivity.current as FragmentActivity
    val app = activity.amanfold
    val scope = rememberCoroutineScope()
    var pw by rememberSaveable { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var failure by remember { mutableStateOf<Throwable?>(null) }
    var notice by remember { mutableStateOf<String?>(null) }
    val quick = app.vault.hasQuickUnlock && app.quickKeys.hasKey()
    val changed = stringResource(R.string.biometric_changed)

    fun tryQuick() {
        scope.launch {
            when (quickUnlock(activity)) {
                QuickResult.Changed -> notice = changed
                else -> Unit
            }
        }
    }
    LaunchedEffect(Unit) { if (quick) tryQuick() }

    Centered {
        Logo()
        Text(stringResource(R.string.locked), style = MaterialTheme.typography.titleLarge)
        app.vault.accountInfo()?.let { (email, _) ->
            Text(email, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        notice?.let { Banner(it) }
        SecretField(pw, { pw = it }, stringResource(R.string.master_password))
        failure?.let { Banner(errorText(it), danger = true) }
        BusyButton(stringResource(R.string.unlock), busy, enabled = pw.isNotEmpty()) {
            busy = true
            failure = null
            scope.launch {
                try {
                    app.vault.unlock(pw)
                    pw = ""
                } catch (e: Throwable) {
                    failure = e
                } finally {
                    busy = false
                }
            }
        }
        if (quick) {
            OutlinedButton(onClick = { tryQuick() }, modifier = Modifier.fillMaxWidth().height(52.dp)) {
                Icon(Icons.Default.Fingerprint, contentDescription = null)
                Text("  " + stringResource(R.string.unlock_biometric))
            }
        }
        if (onSignOut != null) TextButton(onClick = onSignOut) { Text(stringResource(R.string.sign_out)) }
    }
}

/** Offered once after the first unlock on this phone. */
@Composable
fun QuickUnlockOffer(onDone: () -> Unit) {
    val activity = LocalActivity.current as FragmentActivity
    val scope = rememberCoroutineScope()
    AlertDialog(
        onDismissRequest = onDone,
        icon = { Icon(Icons.Default.Fingerprint, contentDescription = null) },
        title = { Text(stringResource(R.string.biometric_enable_title)) },
        text = { Text(stringResource(R.string.biometric_offer)) },
        confirmButton = {
            TextButton(onClick = {
                scope.launch {
                    runCatching { enableQuickUnlock(activity) }
                    onDone()
                }
            }) { Text(stringResource(R.string.turn_on)) }
        },
        dismissButton = { TextButton(onClick = onDone) { Text(stringResource(R.string.not_now)) } },
    )
}
