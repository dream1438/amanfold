package com.amanfold.app.ui

import android.content.Context
import android.print.PrintAttributes
import android.print.PrintManager
import android.text.Html
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Checkbox
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import com.amanfold.app.R
import com.amanfold.app.amanfold
import com.amanfold.client.RecoveryKit
import kotlinx.coroutines.launch

@Composable
fun WelcomeScreen(onCreate: () -> Unit, onSignIn: () -> Unit) {
    Centered {
        Logo()
        Text(stringResource(R.string.tagline), style = MaterialTheme.typography.titleMedium)
        Banner(stringResource(R.string.beta_note))
        Spacer(Modifier.height(8.dp))
        BusyButton(stringResource(R.string.create_account), busy = false, onClick = onCreate)
        OutlinedButton(onClick = onSignIn, modifier = Modifier.fillMaxWidth().height(52.dp)) {
            Text(stringResource(R.string.sign_in))
        }
    }
}

/** The server address, folded away unless the person wants another one. */
@Composable
fun ServerRow(server: String, onChange: (String) -> Unit) {
    var editing by rememberSaveable { mutableStateOf(false) }
    if (editing) {
        Field(server, onChange, stringResource(R.string.server), keyboard = KeyboardType.Uri)
    } else {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                stringResource(R.string.server) + ": " + server,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.weight(1f),
            )
            TextButton(onClick = { editing = true }) { Text(stringResource(R.string.edit)) }
        }
    }
}

@Composable
fun CreateAccountScreen(onBack: () -> Unit, onCreated: (RecoveryKit) -> Unit) {
    val ctx = LocalContext.current
    val app = ctx.amanfold
    val scope = rememberCoroutineScope()
    var server by rememberSaveable { mutableStateOf(app.prefs.server) }
    var email by rememberSaveable { mutableStateOf("") }
    var code by rememberSaveable { mutableStateOf("") }
    var needsCode by rememberSaveable { mutableStateOf<Boolean?>(null) }
    var pw by rememberSaveable { mutableStateOf("") }
    var pw2 by rememberSaveable { mutableStateOf("") }
    var understood by rememberSaveable { mutableStateOf(false) }
    var busy by rememberSaveable { mutableStateOf(false) }
    var error by rememberSaveable { mutableStateOf<String?>(null) }
    var failure by remember { mutableStateOf<Throwable?>(null) }

    Page(stringResource(R.string.create_account), onBack = onBack) {
        Banner(stringResource(R.string.beta_note))
        ServerRow(server) { server = it }
        Field(email, { email = it.trim() }, stringResource(R.string.email), keyboard = KeyboardType.Email)
        if (needsCode == null) {
            BusyButton(stringResource(R.string.continue_), busy, enabled = email.contains('@')) {
                busy = true
                failure = null
                scope.launch {
                    try {
                        val info = app.vault.serverInfo(server)
                        if (info.emailVerification) app.vault.requestSignupCode(server, email)
                        needsCode = info.emailVerification
                        app.prefs.server = server
                    } catch (e: Throwable) {
                        failure = e
                    } finally {
                        busy = false
                    }
                }
            }
        } else {
            if (needsCode == true) {
                Text(stringResource(R.string.code_sent, email), style = MaterialTheme.typography.bodyMedium)
                Field(code, { code = it.filter(Char::isDigit).take(8) }, stringResource(R.string.email_code), keyboard = KeyboardType.NumberPassword)
            }
            SecretField(pw, { pw = it }, stringResource(R.string.master_password), supporting = stringResource(R.string.password_hint))
            SecretField(pw2, { pw2 = it }, stringResource(R.string.confirm_password))
            Row(verticalAlignment = Alignment.Top) {
                Checkbox(understood, { understood = it })
                Text(stringResource(R.string.no_reset), Modifier.padding(top = 12.dp), style = MaterialTheme.typography.bodyMedium)
            }
            error?.let { Banner(it, danger = true) }
            if (busy) Text(stringResource(R.string.creating_keys), style = MaterialTheme.typography.bodySmall)
            val short = stringResource(R.string.password_short)
            val differ = stringResource(R.string.passwords_differ)
            BusyButton(
                stringResource(R.string.create_account),
                busy,
                enabled = understood && pw.isNotEmpty() && (needsCode == false || code.length == 8),
            ) {
                error = when {
                    pw.length < 10 -> short
                    pw != pw2 -> differ
                    else -> null
                }
                if (error != null) return@BusyButton
                busy = true
                failure = null
                scope.launch {
                    try {
                        val kit = app.vault.signup(server, email, pw, code.takeIf { needsCode == true })
                        app.prefs.server = server
                        onCreated(kit)
                    } catch (e: Throwable) {
                        failure = e
                    } finally {
                        busy = false
                    }
                }
            }
        }
        failure?.let { Banner(errorText(it, hadCode = needsCode == true), danger = true) }
    }
}

@Composable
fun SignInScreen(onBack: () -> Unit, onForgot: () -> Unit) {
    val app = LocalContext.current.amanfold
    val scope = rememberCoroutineScope()
    var server by rememberSaveable { mutableStateOf(app.prefs.server) }
    var email by rememberSaveable { mutableStateOf("") }
    var pw by rememberSaveable { mutableStateOf("") }
    var secretKey by rememberSaveable { mutableStateOf("") }
    var busy by rememberSaveable { mutableStateOf(false) }
    var failure by remember { mutableStateOf<Throwable?>(null) }

    Page(stringResource(R.string.sign_in), onBack = onBack) {
        ServerRow(server) { server = it }
        Field(email, { email = it.trim() }, stringResource(R.string.email), keyboard = KeyboardType.Email)
        SecretField(pw, { pw = it }, stringResource(R.string.master_password))
        SecretField(
            secretKey,
            { secretKey = it },
            stringResource(R.string.secret_key),
            supporting = stringResource(R.string.secret_key_hint),
            keyboard = KeyboardType.Ascii,
        )
        failure?.let { Banner(errorText(it), danger = true) }
        BusyButton(stringResource(R.string.sign_in), busy, enabled = email.contains('@') && pw.isNotEmpty() && secretKey.isNotBlank()) {
            busy = true
            failure = null
            scope.launch {
                try {
                    app.vault.signin(server, email, pw, secretKey)
                    app.prefs.server = server
                } catch (e: Throwable) {
                    failure = e
                } finally {
                    busy = false
                }
            }
        }
        TextButton(onClick = onForgot) { Text(stringResource(R.string.forgot_password)) }
    }
}

@Composable
fun RecoverScreen(onBack: () -> Unit) {
    val app = LocalContext.current.amanfold
    val scope = rememberCoroutineScope()
    var server by rememberSaveable { mutableStateOf(app.prefs.server) }
    var email by rememberSaveable { mutableStateOf("") }
    var needsCode by rememberSaveable { mutableStateOf<Boolean?>(null) }
    var code by rememberSaveable { mutableStateOf("") }
    var words by rememberSaveable { mutableStateOf("") }
    var secretKey by rememberSaveable { mutableStateOf("") }
    var pw by rememberSaveable { mutableStateOf("") }
    var pw2 by rememberSaveable { mutableStateOf("") }
    var busy by rememberSaveable { mutableStateOf(false) }
    var error by rememberSaveable { mutableStateOf<String?>(null) }
    var failure by remember { mutableStateOf<Throwable?>(null) }

    Page(stringResource(R.string.recover_title), onBack = onBack) {
        Text(stringResource(R.string.recover_intro), style = MaterialTheme.typography.bodyMedium)
        ServerRow(server) { server = it }
        Field(email, { email = it.trim() }, stringResource(R.string.email), keyboard = KeyboardType.Email)
        if (needsCode == null) {
            BusyButton(stringResource(R.string.continue_), busy, enabled = email.contains('@')) {
                busy = true
                failure = null
                scope.launch {
                    try {
                        val info = app.vault.serverInfo(server)
                        if (info.emailVerification) app.vault.requestRecoveryCode(server, email)
                        needsCode = info.emailVerification
                    } catch (e: Throwable) {
                        failure = e
                    } finally {
                        busy = false
                    }
                }
            }
        } else {
            if (needsCode == true) {
                Text(stringResource(R.string.code_sent, email), style = MaterialTheme.typography.bodyMedium)
                Field(code, { code = it.filter(Char::isDigit).take(8) }, stringResource(R.string.email_code), keyboard = KeyboardType.NumberPassword)
            }
            Field(words, { words = it }, stringResource(R.string.recovery_words), singleLine = false, minLines = 3, style = SecretText)
            SecretField(secretKey, { secretKey = it }, stringResource(R.string.secret_key), keyboard = KeyboardType.Ascii)
            SecretField(pw, { pw = it }, stringResource(R.string.new_master_password), supporting = stringResource(R.string.password_hint))
            SecretField(pw2, { pw2 = it }, stringResource(R.string.confirm_password))
            error?.let { Banner(it, danger = true) }
            val short = stringResource(R.string.password_short)
            val differ = stringResource(R.string.passwords_differ)
            BusyButton(stringResource(R.string.continue_), busy, enabled = words.isNotBlank() && secretKey.isNotBlank() && pw.isNotEmpty()) {
                error = when {
                    pw.length < 10 -> short
                    pw != pw2 -> differ
                    else -> null
                }
                if (error != null) return@BusyButton
                busy = true
                failure = null
                scope.launch {
                    try {
                        app.vault.recover(server, email, code.takeIf { needsCode == true }, words.trim(), secretKey, pw)
                        app.prefs.server = server
                    } catch (e: Throwable) {
                        failure = e
                    } finally {
                        busy = false
                    }
                }
            }
        }
        failure?.let { Banner(errorText(it, hadCode = needsCode == true), danger = true) }
    }
}

@Composable
fun RecoveryKitScreen(kit: RecoveryKit, onDone: () -> Unit) {
    val ctx = LocalContext.current
    var saved by rememberSaveable { mutableStateOf(false) }
    Page(stringResource(R.string.kit_title)) {
        Text(stringResource(R.string.kit_intro), style = MaterialTheme.typography.bodyMedium)
        KitValue(stringResource(R.string.email), kit.email)
        KitValue(stringResource(R.string.secret_key), kit.secretKey, secret = true)
        KitValue(stringResource(R.string.recovery_words), numbered(kit.recoveryWords), secret = true)
        val title = stringResource(R.string.kit_file_title)
        val warn = stringResource(R.string.kit_file_warn)
        val labels = listOf(stringResource(R.string.email), stringResource(R.string.secret_key), stringResource(R.string.recovery_words))
        OutlinedButton(onClick = { printKit(ctx, kit, title, warn, labels) }, modifier = Modifier.fillMaxWidth()) {
            Text(stringResource(R.string.print_kit))
        }
        Row(verticalAlignment = Alignment.CenterVertically) {
            Checkbox(saved, { saved = it })
            Text(stringResource(R.string.kit_saved_check))
        }
        BusyButton(stringResource(R.string.continue_), busy = false, enabled = saved, onClick = onDone)
    }
}

private fun numbered(words: String): String =
    words.trim().split(Regex("\\s+")).mapIndexed { i, w -> "${i + 1}. $w" }.chunked(3).joinToString("\n") { it.joinToString("   ") }

@Composable
private fun KitValue(label: String, value: String, secret: Boolean = false) {
    Surface(shape = RoundedCornerShape(12.dp), color = MaterialTheme.colorScheme.surface, modifier = Modifier.fillMaxWidth()) {
        androidx.compose.foundation.layout.Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(label, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            // Left to right even on Arabic screens: these are Latin codes.
            androidx.compose.runtime.CompositionLocalProvider(
                androidx.compose.ui.platform.LocalLayoutDirection provides androidx.compose.ui.unit.LayoutDirection.Ltr,
            ) {
                Text(value, style = if (secret) SecretText else MaterialTheme.typography.bodyLarge)
            }
        }
    }
}

private var printing: WebView? = null

/** Prints the kit (or saves it as a PDF) through the system print dialog. Nothing is written to app storage. */
private fun printKit(ctx: Context, kit: RecoveryKit, title: String, warn: String, labels: List<String>) {
    fun esc(s: String) = Html.escapeHtml(s)
    val words = kit.recoveryWords.trim().split(Regex("\\s+"))
        .map { w -> "<li>${esc(w)}</li>" }.joinToString("")
    val html = """
        <html><head><meta charset="utf-8"><style>
        body{font-family:sans-serif;color:#0C1F1A;margin:32px}
        h1{color:#0B6E4F;font-size:22px} .warn{border:2px solid #B3261E;padding:10px;border-radius:8px}
        .v{font-family:monospace;font-size:18px;direction:ltr} ol{columns:3;font-family:monospace;font-size:15px;direction:ltr}
        </style></head><body>
        <h1>${esc(title)}</h1><p class="warn">${esc(warn)}</p>
        <h3>${esc(labels[0])}</h3><p class="v">${esc(kit.email)}</p>
        <h3>${esc(labels[1])}</h3><p class="v">${esc(kit.secretKey)}</p>
        <h3>${esc(labels[2])}</h3><ol>$words</ol>
        </body></html>
    """.trimIndent()
    val web = WebView(ctx)
    printing = web // keep it alive until the print job has its content
    web.webViewClient = object : WebViewClient() {
        override fun onPageFinished(view: WebView, url: String?) {
            val pm = ctx.getSystemService(PrintManager::class.java)
            pm.print(title, view.createPrintDocumentAdapter(title), PrintAttributes.Builder().build())
        }
    }
    web.settings.javaScriptEnabled = false
    web.loadDataWithBaseURL(null, html, "text/html", "utf-8", null)
}
