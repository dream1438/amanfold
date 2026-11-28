package com.amanfold.app.autofill

import android.os.Bundle
import android.view.WindowManager
import androidx.activity.compose.setContent
import androidx.appcompat.app.AppCompatActivity
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import com.amanfold.app.R
import com.amanfold.app.amanfold
import com.amanfold.app.ui.AmanfoldTheme
import com.amanfold.app.ui.Banner
import com.amanfold.app.ui.Centered
import com.amanfold.app.ui.Logo
import com.amanfold.app.ui.UnlockScreen
import com.amanfold.app.ui.errorText
import com.amanfold.app.ui.value
import com.amanfold.client.VaultState
import com.amanfold.client.autofill.FillTarget
import com.amanfold.core.FieldInput
import com.amanfold.core.ItemKind
import com.amanfold.core.ItemView
import com.amanfold.core.cardInfo
import com.amanfold.core.pageSite
import kotlinx.coroutines.launch

/** Asks whether to save (or update) what the person just typed into a form. */
class SaveActivity : AppCompatActivity() {
    companion object {
        const val EXTRA_TOKEN = "token"
    }

    private var save: PendingSave? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.setFlags(WindowManager.LayoutParams.FLAG_SECURE, WindowManager.LayoutParams.FLAG_SECURE)
        save = PendingSave.take(intent.getStringExtra(EXTRA_TOKEN))
        if (save == null) return finish()
        setContent {
            AmanfoldTheme {
                val state by amanfold.vault.state.collectAsState()
                when (state) {
                    VaultState.NoAccount -> finish()
                    VaultState.Locked -> UnlockScreen(onSignOut = null)
                    VaultState.Unlocked -> Prompt(save!!)
                }
            }
        }
    }

    @Composable
    private fun Prompt(s: PendingSave) {
        val vault = amanfold.vault
        val scope = rememberCoroutineScope()
        var failure by remember { mutableStateOf<Throwable?>(null) }
        var busy by remember { mutableStateOf(false) }

        // What the form is for, and whether this login already exists here.
        val plan = remember {
            when (s) {
                is PendingSave.Login -> {
                    val (where, url) = when (val t = s.target) {
                        is FillTarget.Web -> (pageSite(t.url)?.displayHost ?: t.host) to t.url
                        is FillTarget.App -> appLabel(this, t.packageName) to appIdentityUrl(this, t.packageName)
                    }
                    val existing = url?.let {
                        val m = if (s.target is FillTarget.Web) vault.loginsForPage(it) else vault.loginsForApp(it)
                        m.map { x -> x.item }.firstOrNull { i -> i.value("username") == s.username }
                    }
                    Triple(where, url, existing)
                }
                is PendingSave.Card -> Triple("", null, null)
            }
        }
        val (where, url, existing) = plan
        val unchanged = existing?.value("password") == (s as? PendingSave.Login)?.password
        if (unchanged) {
            finish()
            return
        }

        Centered {
            Logo()
            Text(
                when {
                    s is PendingSave.Card -> stringResource(R.string.save_card_prompt)
                    existing != null -> stringResource(R.string.update_prompt, where)
                    else -> stringResource(R.string.save_prompt, where)
                },
                style = MaterialTheme.typography.titleMedium,
            )
            if (s is PendingSave.Login && s.username != null) Text(s.username, style = MaterialTheme.typography.bodyMedium)
            failure?.let { Banner(errorText(it), danger = true) }
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                OutlinedButton(onClick = { finish() }) { Text(stringResource(R.string.not_now)) }
                Button(enabled = !busy, onClick = {
                    busy = true
                    scope.launch {
                        try {
                            store(s, where, url, existing)
                            finish()
                        } catch (e: Throwable) {
                            failure = e
                            busy = false
                        }
                    }
                }) { Text(stringResource(if (existing != null) R.string.update else R.string.save)) }
            }
        }
    }

    private suspend fun store(s: PendingSave, where: String, url: String?, existing: ItemView?) {
        val vault = amanfold.vault
        when (s) {
            is PendingSave.Login -> if (existing != null) {
                vault.update(existing.id, listOf(FieldInput("password", s.password)))
            } else {
                vault.create(
                    ItemKind.LOGIN,
                    listOfNotNull(
                        FieldInput("title", where),
                        s.username?.let { FieldInput("username", it) },
                        FieldInput("password", s.password),
                        url?.let { FieldInput("url", it) },
                    ),
                )
            }
            is PendingSave.Card -> {
                val info = cardInfo(s.number)
                val (month, year) = expiry(s)
                vault.create(
                    ItemKind.CARD,
                    listOfNotNull(
                        FieldInput("title", "${info.brand.name.lowercase().replaceFirstChar { it.uppercase() }} •••• ${info.digits.takeLast(4)}"),
                        FieldInput("card_number", info.digits),
                        s.holder?.let { FieldInput("cardholder", it) },
                        month?.let { FieldInput("exp_month", it) },
                        year?.let { FieldInput("exp_year", it) },
                        s.cvv?.let { FieldInput("cvv", it) },
                    ),
                )
            }
        }
    }

    /** Month and four-digit year from separate fields or one "MM/YY" field. */
    private fun expiry(s: PendingSave.Card): Pair<String?, String?> {
        fun year(y: String) = if (y.length == 2) "20$y" else y
        val m = s.month?.filter(Char::isDigit)?.takeIf { it.isNotEmpty() }
        val y = s.year?.filter(Char::isDigit)?.takeIf { it.isNotEmpty() }
        if (m != null || y != null) return m to y?.let(::year)
        val parts = s.expiry?.split('/', '-', ' ')?.map { it.filter(Char::isDigit) }?.filter { it.isNotEmpty() } ?: return null to null
        if (parts.size != 2) return null to null
        return parts[0] to year(parts[1])
    }
}
