package com.amanfold.app.autofill

import android.app.Activity
import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.service.autofill.Dataset
import android.view.WindowManager
import android.view.autofill.AutofillId
import android.view.autofill.AutofillManager
import android.view.autofill.AutofillValue
import android.widget.RemoteViews
import androidx.activity.compose.setContent
import androidx.appcompat.app.AppCompatActivity
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Checkbox
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import com.amanfold.app.R
import com.amanfold.app.amanfold
import com.amanfold.app.ui.AmanfoldTheme
import com.amanfold.app.ui.Banner
import com.amanfold.app.ui.Centered
import com.amanfold.app.ui.Page
import com.amanfold.app.ui.UnlockScreen
import com.amanfold.app.ui.subtitle
import com.amanfold.app.ui.titleText
import com.amanfold.app.ui.value
import com.amanfold.client.VaultState
import com.amanfold.client.autofill.CARD_ROLES
import com.amanfold.client.autofill.FieldRole
import com.amanfold.core.FieldInput
import com.amanfold.core.ItemKind
import com.amanfold.core.ItemView
import com.amanfold.core.pageSite
import com.amanfold.core.totpNow
import kotlinx.coroutines.launch

/**
 * Opened when the person taps "Fill with Amanfold". Unlocks if needed, offers
 * only what may be filled here (logins for this site or app, by the core's
 * matching; cards), and returns the chosen values to the system.
 */
class FillActivity : AppCompatActivity() {
    companion object {
        const val EXTRA_IDS = "ids"
        const val EXTRA_ROLES = "roles"
        const val EXTRA_URL = "url"
        const val EXTRA_HOST = "host"
        const val EXTRA_PACKAGE = "package"
        const val EXTRA_CARD_FRAME = "cardFrame"
    }

    private lateinit var fields: List<Pair<AutofillId, FieldRole>>
    private var url: String? = null
    private var host: String? = null
    private var pkg: String? = null
    private var appUrl: String? = null
    private var cardFrame: String? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.setFlags(WindowManager.LayoutParams.FLAG_SECURE, WindowManager.LayoutParams.FLAG_SECURE)
        val ids: List<AutofillId> = if (Build.VERSION.SDK_INT >= 33) {
            intent.getParcelableArrayListExtra(EXTRA_IDS, AutofillId::class.java)
        } else {
            @Suppress("DEPRECATION")
            intent.getParcelableArrayListExtra(EXTRA_IDS)
        } ?: emptyList()
        val roles = intent.getIntArrayExtra(EXTRA_ROLES) ?: IntArray(0)
        if (ids.isEmpty() || ids.size != roles.size) return cancel()
        fields = ids.zip(roles.map { FieldRole.entries[it] })
        url = intent.getStringExtra(EXTRA_URL)
        host = intent.getStringExtra(EXTRA_HOST)
        pkg = intent.getStringExtra(EXTRA_PACKAGE)
        cardFrame = intent.getStringExtra(EXTRA_CARD_FRAME)
        pkg?.let { appUrl = appIdentityUrl(this, it) }

        setContent {
            AmanfoldTheme {
                val state by amanfold.vault.state.collectAsState()
                when (state) {
                    VaultState.NoAccount -> Centered {
                        Text(stringResource(R.string.fill_locked))
                        TextButton(onClick = { cancel() }) { Text(stringResource(R.string.cancel)) }
                    }
                    VaultState.Locked -> UnlockScreen(onSignOut = null)
                    VaultState.Unlocked -> Picker()
                }
            }
        }
    }

    private fun cancel() {
        setResult(Activity.RESULT_CANCELED)
        finish()
    }

    private val wantsLogin get() = fields.any { it.second in setOf(FieldRole.Username, FieldRole.Password, FieldRole.OneTimeCode) }
    private val wantsCard get() = fields.any { it.second in CARD_ROLES }

    @Composable
    private fun Picker() {
        val vault = amanfold.vault
        val scope = rememberCoroutineScope()
        var showAll by remember { mutableStateOf(false) }
        var confirm by remember { mutableStateOf<ItemView?>(null) }
        var confirmCard by remember { mutableStateOf<ItemView?>(null) }

        val matches = remember {
            when {
                url != null -> vault.loginsForPage(url!!).map { it.item }
                appUrl != null -> vault.loginsForApp(appUrl!!).map { it.item }
                else -> emptyList()
            }
        }
        val lookalikes = remember { url?.let(vault::lookalikes) ?: emptyList() }
        val site = remember { url?.let(::pageSite) }
        val label = remember { pkg?.let { appLabel(this, it) } }
        val logins = remember { vault.items().filter { it.kind == ItemKind.LOGIN } }
        val cards = remember { vault.items().filter { it.kind == ItemKind.CARD } }

        Page(
            title = when {
                url != null -> stringResource(R.string.for_this_site, site?.displayHost ?: host ?: "")
                else -> stringResource(R.string.for_this_app)
            },
            onBack = { cancel() },
        ) {
            if (label != null) Text(stringResource(R.string.app_is, "$label ($pkg)"), style = MaterialTheme.typography.bodySmall)
            if (site != null && site.displayHost != site.host) Banner(stringResource(R.string.punycode, site.host), danger = true)
            if (site != null && !site.secure) Banner(stringResource(R.string.insecure), danger = true)
            lookalikes.firstOrNull()?.let { Banner(stringResource(R.string.lookalike, it), danger = true) }

            if (wantsLogin) {
                if (matches.isEmpty()) {
                    Text(stringResource(if (url != null) R.string.none_for_site else R.string.none_for_app), color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                matches.forEach { ItemChoice(it) { fillLogin(it) } }
                // Picking another login needs a yes, and never on a look-alike page.
                if (lookalikes.isEmpty()) {
                    if (!showAll) {
                        TextButton(onClick = { showAll = true }) { Text(stringResource(R.string.search_all)) }
                    } else {
                        logins.filter { l -> matches.none { it.id == l.id } }.forEach { ItemChoice(it) { confirm = it } }
                    }
                }
            }
            if (wantsCard) {
                Text(stringResource(R.string.cards), style = MaterialTheme.typography.titleSmall)
                cards.forEach { ItemChoice(it) { if (cardFrame != null) confirmCard = it else fillCard(it) } }
            }
        }

        confirm?.let { item ->
            var link by remember { mutableStateOf(false) }
            AlertDialog(
                onDismissRequest = { confirm = null },
                text = {
                    Column {
                        Text(
                            if (url != null) stringResource(R.string.confirm_any_site, site?.displayHost ?: host ?: "")
                            else stringResource(R.string.confirm_any_app, label ?: pkg ?: ""),
                        )
                        if (appUrl != null) {
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Checkbox(link, { link = it })
                                Text(stringResource(R.string.remember_app))
                            }
                        }
                    }
                },
                confirmButton = {
                    TextButton(onClick = {
                        val linkIt = link
                        confirm = null
                        scope.launch {
                            if (linkIt && appUrl != null) linkApp(item, appUrl!!)
                            fillLogin(item)
                        }
                    }) { Text(stringResource(R.string.fill_anyway)) }
                },
                dismissButton = { TextButton(onClick = { confirm = null }) { Text(stringResource(R.string.cancel)) } },
            )
        }
        confirmCard?.let { card ->
            AlertDialog(
                onDismissRequest = { confirmCard = null },
                text = { Text(stringResource(R.string.confirm_frame, cardFrame ?: "")) },
                confirmButton = { TextButton(onClick = { confirmCard = null; fillCard(card) }) { Text(stringResource(R.string.fill)) } },
                dismissButton = { TextButton(onClick = { confirmCard = null }) { Text(stringResource(R.string.cancel)) } },
            )
        }
    }

    @Composable
    private fun ItemChoice(item: ItemView, onClick: () -> Unit) {
        Column(
            Modifier
                .fillMaxWidth()
                .clickable(onClick = onClick)
                .padding(vertical = 12.dp),
        ) {
            Text(item.titleText(), style = MaterialTheme.typography.titleMedium)
            val sub = item.subtitle()
            if (sub.isNotEmpty()) Text(sub, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
    }

    private suspend fun linkApp(item: ItemView, appUrl: String) {
        val used = item.fields.map { it.name }.toSet()
        val name = if ("url" !in used) "url" else (2..99).map { "url.$it" }.first { it !in used }
        runCatching { amanfold.vault.update(item.id, listOf(FieldInput(name, appUrl))) }
    }

    private fun fillLogin(item: ItemView) {
        val totp = item.value("totp")?.let { runCatching { totpNow(it).code }.getOrNull() }
        respond(item) { role ->
            when (role) {
                FieldRole.Username -> item.value("username")
                FieldRole.Password -> item.value("password")
                FieldRole.OneTimeCode -> totp
                else -> null
            }
        }
    }

    private fun fillCard(item: ItemView) {
        val month = item.value("exp_month")?.padStart(2, '0')
        val year = item.value("exp_year")
        respond(item) { role ->
            when (role) {
                FieldRole.CardNumber -> item.value("card_number")
                FieldRole.CardHolder -> item.value("cardholder")
                FieldRole.CardCvv -> item.value("cvv")
                FieldRole.CardExpMonth -> month
                FieldRole.CardExpYear -> year
                FieldRole.CardExpiry -> if (month != null && year != null) "$month/${year.takeLast(2)}" else null
                else -> null
            }
        }
    }

    private fun respond(item: ItemView, valueFor: (FieldRole) -> String?) {
        val presentation = RemoteViews(packageName, R.layout.autofill_item).apply {
            setTextViewText(R.id.text, item.value("title") ?: getString(R.string.app_name))
        }
        @Suppress("DEPRECATION")
        val builder = Dataset.Builder(presentation)
        var any = false
        for ((id, role) in fields) {
            val v = valueFor(role) ?: continue
            @Suppress("DEPRECATION")
            builder.setValue(id, AutofillValue.forText(v), presentation)
            any = true
        }
        if (!any) return cancel()
        setResult(Activity.RESULT_OK, Intent().putExtra(AutofillManager.EXTRA_AUTHENTICATION_RESULT, builder.build()))
        finish()
    }
}
