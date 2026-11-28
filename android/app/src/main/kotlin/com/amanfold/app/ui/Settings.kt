package com.amanfold.app.ui

import androidx.activity.compose.LocalActivity
import android.content.Intent
import android.net.Uri
import android.provider.Settings
import android.text.format.DateUtils
import android.view.autofill.AutofillManager
import androidx.appcompat.app.AppCompatDelegate
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Checkbox
import androidx.compose.material3.FilterChip
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Slider
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.core.os.LocaleListCompat
import androidx.fragment.app.FragmentActivity
import androidx.lifecycle.compose.LifecycleResumeEffect
import com.amanfold.app.BuildConfig
import com.amanfold.app.Clipboard
import com.amanfold.app.Prefs
import com.amanfold.app.R
import com.amanfold.app.amanfold
import com.amanfold.core.PasswordRules
import com.amanfold.core.coreVersion
import com.amanfold.core.defaultPasswordRules
import com.amanfold.core.generatePassphrase
import com.amanfold.core.generatePassword
import kotlinx.coroutines.launch

@Composable
private fun Section(title: String, content: @Composable () -> Unit) {
    Text(title, style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.primary, modifier = Modifier.padding(top = 8.dp))
    Surface(shape = RoundedCornerShape(12.dp), color = MaterialTheme.colorScheme.surface, modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) { content() }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun SettingsScreen(onBack: () -> Unit) {
    val activity = LocalActivity.current as FragmentActivity
    val app = activity.amanfold
    val scope = rememberCoroutineScope()
    val sync by app.vault.syncStatus.collectAsState()
    var autoLock by remember { mutableIntStateOf(app.prefs.autoLockMinutes) }
    var quick by remember { mutableStateOf(app.vault.hasQuickUnlock && app.quickKeys.hasKey()) }
    var autofillOn by remember { mutableStateOf(false) }
    var confirmSignOut by remember { mutableStateOf(false) }
    var syncing by remember { mutableStateOf(false) }
    var failure by remember { mutableStateOf<Throwable?>(null) }

    LifecycleResumeEffect(Unit) {
        autofillOn = activity.getSystemService(AutofillManager::class.java)?.hasEnabledAutofillServices() == true
        onPauseOrDispose { }
    }

    Page(stringResource(R.string.settings), onBack = onBack) {
        Section(stringResource(R.string.autofill)) {
            Text(stringResource(if (autofillOn) R.string.autofill_on else R.string.autofill_off), style = MaterialTheme.typography.bodyMedium)
            if (!autofillOn) {
                OutlinedButton(onClick = {
                    activity.startActivity(
                        Intent(Settings.ACTION_REQUEST_SET_AUTOFILL_SERVICE, Uri.parse("package:" + activity.packageName)),
                    )
                }) { Text(stringResource(R.string.autofill_turn_on)) }
            }
        }

        Section(stringResource(R.string.lock)) {
            Text(stringResource(R.string.auto_lock), style = MaterialTheme.typography.bodyMedium)
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                for (m in Prefs.AUTO_LOCK_CHOICES) {
                    val label = when {
                        m == 0 -> stringResource(R.string.immediately)
                        m >= 60 -> stringResource(R.string.hours, m / 60)
                        else -> stringResource(R.string.minutes, m)
                    }
                    FilterChip(selected = autoLock == m, onClick = { autoLock = m; app.prefs.autoLockMinutes = m }, label = { Text(label) })
                }
            }
            if (app.quickKeys.available() || quick) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(stringResource(R.string.quick_unlock), Modifier.weight(1f))
                    Switch(checked = quick, onCheckedChange = { on ->
                        if (on) {
                            scope.launch { quick = runCatching { enableQuickUnlock(activity) }.getOrDefault(false) }
                        } else {
                            disableQuickUnlock(activity)
                            quick = false
                        }
                    })
                }
            }
        }

        Section(stringResource(R.string.language)) {
            val current = AppCompatDelegate.getApplicationLocales().toLanguageTags()
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                for ((tag, label) in listOf("" to stringResource(R.string.lang_system), "en" to "English", "ar" to "العربية")) {
                    FilterChip(
                        selected = current.substringBefore('-') == tag,
                        onClick = { AppCompatDelegate.setApplicationLocales(LocaleListCompat.forLanguageTags(tag)) },
                        label = { Text(label) },
                    )
                }
            }
        }

        Section(stringResource(R.string.account)) {
            app.vault.accountInfo()?.let { (email, server) ->
                Text(email, style = MaterialTheme.typography.bodyLarge)
                Text(server, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            Text(
                sync.lastSyncMs?.let { stringResource(R.string.last_sync, DateUtils.getRelativeTimeSpanString(it).toString()) }
                    ?: stringResource(R.string.never_synced),
                style = MaterialTheme.typography.bodySmall,
            )
            failure?.let { Banner(errorText(it), danger = true) }
            OutlinedButton(enabled = !syncing, onClick = {
                syncing = true
                failure = null
                scope.launch {
                    try { app.vault.sync() } catch (e: Throwable) { failure = e } finally { syncing = false }
                }
            }) { Text(stringResource(if (syncing) R.string.working else R.string.sync_now)) }
            TextButton(onClick = { confirmSignOut = true }) {
                Text(stringResource(R.string.sign_out), color = MaterialTheme.colorScheme.error)
            }
        }

        Banner(stringResource(R.string.beta_note))
        Text(
            stringResource(R.string.version, "${BuildConfig.VERSION_NAME} (core ${coreVersion()})"),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }

    if (confirmSignOut) {
        AlertDialog(
            onDismissRequest = { confirmSignOut = false },
            text = { Text(stringResource(R.string.sign_out_confirm)) },
            confirmButton = {
                TextButton(onClick = {
                    confirmSignOut = false
                    scope.launch {
                        disableQuickUnlock(activity)
                        app.prefs.clearAccountState()
                        app.vault.signout()
                    }
                }) { Text(stringResource(R.string.sign_out)) }
            },
            dismissButton = { TextButton(onClick = { confirmSignOut = false }) { Text(stringResource(R.string.cancel)) } },
        )
    }
}

/** The generator. With [onUse], it fills a password field being edited. */
@Composable
fun GeneratorScreen(onBack: () -> Unit, onUse: ((String) -> Unit)?) {
    val ctx = LocalContext.current
    val d = remember { defaultPasswordRules() }
    var length by rememberSaveable { mutableIntStateOf(d.length.toInt()) }
    var lower by rememberSaveable { mutableStateOf(d.lowercase) }
    var upper by rememberSaveable { mutableStateOf(d.uppercase) }
    var digits by rememberSaveable { mutableStateOf(d.digits) }
    var symbols by rememberSaveable { mutableStateOf(d.symbols) }
    var words by rememberSaveable { mutableStateOf(false) }
    var wordCount by rememberSaveable { mutableIntStateOf(5) }
    var nonce by remember { mutableIntStateOf(0) }
    val value = remember(length, lower, upper, digits, symbols, words, wordCount, nonce) {
        runCatching {
            if (words) {
                generatePassphrase(wordCount.toUInt(), "-")
            } else {
                generatePassword(PasswordRules(length.toUInt(), lower, upper, digits, symbols, d.avoidAmbiguous))
            }
        }.getOrDefault("")
    }
    var copiedNote by remember { mutableStateOf(false) }
    LaunchedEffect(value) { copiedNote = false }

    Page(stringResource(R.string.generator), onBack = onBack) {
        Surface(shape = RoundedCornerShape(12.dp), color = MaterialTheme.colorScheme.surface, modifier = Modifier.fillMaxWidth()) {
            androidx.compose.runtime.CompositionLocalProvider(LocalLayoutDirection provides LayoutDirection.Ltr) {
                Text(value, Modifier.padding(16.dp), style = SecretText)
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedButton(onClick = { nonce++ }) { Text(stringResource(R.string.generate)) }
            OutlinedButton(onClick = { Clipboard.copy(ctx, value, sensitive = true); copiedNote = true }) { Text(stringResource(R.string.copy)) }
            if (onUse != null) {
                androidx.compose.material3.Button(onClick = { onUse(value) }, enabled = value.isNotEmpty()) { Text(stringResource(R.string.use)) }
            }
        }
        if (copiedNote) Text(stringResource(R.string.copied), style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.primary)
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(stringResource(R.string.passphrase), Modifier.weight(1f))
            Switch(words, { words = it })
        }
        if (words) {
            Text(stringResource(R.string.words, wordCount))
            Slider(wordCount.toFloat(), { wordCount = it.toInt() }, valueRange = 3f..10f, steps = 6)
        } else {
            Text(stringResource(R.string.length, length))
            Slider(length.toFloat(), { length = it.toInt() }, valueRange = 8f..64f)
            for ((label, state) in listOf(
                R.string.lowercase to (lower to { v: Boolean -> lower = v }),
                R.string.uppercase to (upper to { v: Boolean -> upper = v }),
                R.string.digits to (digits to { v: Boolean -> digits = v }),
                R.string.symbols to (symbols to { v: Boolean -> symbols = v }),
            )) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Checkbox(state.first, { state.second(it) })
                    Text(stringResource(label))
                }
            }
        }
    }
}
