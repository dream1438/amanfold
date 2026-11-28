package com.amanfold.app.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.CreditCard
import androidx.compose.material.icons.filled.Key
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.Password
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Groups
import androidx.compose.material.icons.automirrored.filled.StickyNote2
import androidx.compose.material.icons.filled.Visibility
import androidx.compose.material.icons.filled.VisibilityOff
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import com.amanfold.app.Clipboard
import com.amanfold.app.R
import com.amanfold.app.amanfold
import com.amanfold.client.SyncProblem
import com.amanfold.client.VaultTag
import com.amanfold.core.FieldInput
import com.amanfold.core.ItemKind
import com.amanfold.core.ItemView
import com.amanfold.core.androidAppPackage
import com.amanfold.core.cardInfo
import com.amanfold.core.totpNow
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

// ---------------------------------------------------------------------------
// Item helpers
// ---------------------------------------------------------------------------

fun ItemView.value(name: String): String? = fields.firstOrNull { it.name == name }?.value?.takeIf { it.isNotEmpty() }

@Composable
fun ItemView.titleText(): String = value("title") ?: stringResource(R.string.untitled)

fun ItemView.subtitle(): String = when (kind) {
    ItemKind.LOGIN -> listOfNotNull(value("username"), value("url")?.let(::shortUrl)).joinToString(" · ")
    ItemKind.CARD -> value("card_number")?.let { "•••• " + it.takeLast(4) } ?: ""
    else -> value("notes")?.lineSequence()?.firstOrNull()?.take(60) ?: ""
}

fun shortUrl(u: String): String =
    androidAppPackage(u) ?: u.removePrefix("https://").removePrefix("http://").substringBefore('/')

fun ItemView.icon(): ImageVector = when (kind) {
    ItemKind.LOGIN -> Icons.Default.Key
    ItemKind.CARD -> Icons.Default.CreditCard
    ItemKind.PASSKEY -> Icons.Default.Password
    else -> Icons.AutoMirrored.Filled.StickyNote2
}

/** Values in these fields are Latin codes; keep them left to right on Arabic screens. */
private val ltrFields = setOf("username", "password", "url", "totp", "card_number", "cvv", "pin", "exp_month", "exp_year")

private enum class Filter { All, Logins, Cards, Notes, Trash }

// ---------------------------------------------------------------------------
// List
// ---------------------------------------------------------------------------

@Composable
fun VaultListScreen(onOpen: (String) -> Unit, onAdd: (ItemKind) -> Unit, onSettings: () -> Unit, onGenerator: () -> Unit, onSharing: () -> Unit) {
    val app = LocalContext.current.amanfold
    val revision by app.vault.revision.collectAsState()
    val sync by app.vault.syncStatus.collectAsState()
    var query by rememberSaveable { mutableStateOf("") }
    var filter by rememberSaveable { mutableStateOf(Filter.All) }
    var addMenu by remember { mutableStateOf(false) }

    // Invitations and vaults waiting for a safety-number check.
    var asks by remember { mutableStateOf(0) }
    LaunchedEffect(Unit) {
        runCatching { app.vault.sync() }
        asks = runCatching { app.vault.sharing().let { it.invites.size + it.waiting.size } }.getOrDefault(0)
    }

    val tags = remember(revision) { if (app.vault.isUnlocked) app.vault.vaults().filter { it.shared }.size else 0 }
    val items = remember(revision, query, filter) {
        if (!app.vault.isUnlocked) return@remember emptyList()
        val base = if (filter == Filter.Trash) app.vault.items(trashed = true) else app.vault.items()
        base.filter {
            when (filter) {
                Filter.Logins -> it.kind == ItemKind.LOGIN
                Filter.Cards -> it.kind == ItemKind.CARD
                Filter.Notes -> it.kind == ItemKind.SECURE_NOTE
                else -> true
            }
        }.filter { q ->
            query.isBlank() || listOf("title", "username", "url", "notes", "cardholder").any { n ->
                q.value(n)?.contains(query.trim(), ignoreCase = true) == true
            }
        }
    }

    Page(
        title = stringResource(R.string.app_name),
        scroll = false,
        actions = {
            IconButton(onClick = onSharing) { Icon(Icons.Default.Groups, stringResource(R.string.sharing)) }
            IconButton(onClick = onGenerator) { Icon(Icons.Default.Password, stringResource(R.string.generator)) }
            IconButton(onClick = { app.vault.lock() }) { Icon(Icons.Default.Lock, stringResource(R.string.lock)) }
            IconButton(onClick = onSettings) { Icon(Icons.Default.Settings, stringResource(R.string.settings)) }
        },
        floating = {
            Column {
                ExtendedFloatingActionButton(
                    onClick = { addMenu = true },
                    icon = { Icon(Icons.Default.Add, null) },
                    text = { Text(stringResource(R.string.add)) },
                )
                DropdownMenu(addMenu, { addMenu = false }) {
                    DropdownMenuItem({ Text(stringResource(R.string.new_login)) }, { addMenu = false; onAdd(ItemKind.LOGIN) }, leadingIcon = { Icon(Icons.Default.Key, null) })
                    DropdownMenuItem({ Text(stringResource(R.string.new_card)) }, { addMenu = false; onAdd(ItemKind.CARD) }, leadingIcon = { Icon(Icons.Default.CreditCard, null) })
                    DropdownMenuItem({ Text(stringResource(R.string.new_note)) }, { addMenu = false; onAdd(ItemKind.SECURE_NOTE) }, leadingIcon = { Icon(Icons.AutoMirrored.Filled.StickyNote2, null) })
                }
            }
        },
    ) {
        Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            when (sync.problem) {
                SyncProblem.Rollback -> Banner(stringResource(R.string.err_rollback), danger = true)
                SyncProblem.SignedOut -> Banner(stringResource(R.string.err_signed_out), danger = true)
                SyncProblem.Offline -> Banner(stringResource(R.string.sync_offline))
                else -> Unit
            }
            if (asks > 0) {
                OutlinedButton(onClick = onSharing, Modifier.fillMaxWidth()) { Text(stringResource(R.string.sharing_asks, asks)) }
            }
            OutlinedTextField(
                value = query,
                onValueChange = { query = it },
                placeholder = { Text(stringResource(R.string.search)) },
                leadingIcon = { Icon(Icons.Default.Search, null) },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
            LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                val chips = listOf(
                    Filter.All to R.string.all_items, Filter.Logins to R.string.logins, Filter.Cards to R.string.cards,
                    Filter.Notes to R.string.notes_filter, Filter.Trash to R.string.trash,
                )
                items(chips) { (f, label) ->
                    FilterChip(selected = filter == f, onClick = { filter = f }, label = { Text(stringResource(label)) })
                }
            }
        }
        if (items.isEmpty()) {
            val msg = when {
                filter == Filter.Trash -> R.string.trash_empty
                query.isNotBlank() || filter != Filter.All -> R.string.no_results
                else -> R.string.empty
            }
            Text(stringResource(msg), Modifier.padding(24.dp), color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 96.dp)) {
            items(items, key = { it.id }) { item ->
                val vault = if (tags > 0) remember(revision, item.id) { app.vault.vaultOf(item.id) } else null
                ItemRow(item, vault) { onOpen(item.id) }
                HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
            }
        }
    }
}

@Composable
private fun ItemRow(item: ItemView, vault: VaultTag?, onClick: () -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 20.dp, vertical = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Surface(shape = RoundedCornerShape(10.dp), color = MaterialTheme.colorScheme.primaryContainer) {
            Icon(item.icon(), null, Modifier.padding(8.dp).size(22.dp), tint = MaterialTheme.colorScheme.onPrimaryContainer)
        }
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text(item.titleText(), style = MaterialTheme.typography.titleMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
            val sub = item.subtitle()
            if (sub.isNotEmpty()) {
                Text(sub, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
        if (vault != null && vault.shared) {
            Spacer(Modifier.width(8.dp))
            VaultChip(vault)
        }
    }
}

@Composable
fun VaultChip(vault: VaultTag) {
    Surface(shape = RoundedCornerShape(50), color = MaterialTheme.colorScheme.secondaryContainer) {
        Row(Modifier.padding(horizontal = 8.dp, vertical = 3.dp), verticalAlignment = Alignment.CenterVertically) {
            Icon(vault.kind.icon(), null, Modifier.size(14.dp), tint = MaterialTheme.colorScheme.onSecondaryContainer)
            Spacer(Modifier.width(4.dp))
            Text(
                vaultName(vault.name),
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSecondaryContainer,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.widthIn(max = 110.dp),
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Detail
// ---------------------------------------------------------------------------

@Composable
fun ItemDetailScreen(id: String, onBack: () -> Unit, onEdit: () -> Unit) {
    val ctx = LocalContext.current
    val app = ctx.amanfold
    val scope = rememberCoroutineScope()
    val revision by app.vault.revision.collectAsState()
    val item = remember(revision, id) { if (app.vault.isUnlocked) app.vault.item(id) else null }
    val vault = remember(revision, id) { if (app.vault.isUnlocked) app.vault.vaultOf(id) else null }
    val writable = vault?.readOnly != true
    val targets = remember(revision, id) {
        if (app.vault.isUnlocked) app.vault.vaults().filter { it.id != vault?.id && !it.readOnly } else emptyList()
    }
    var confirmTrash by remember { mutableStateOf(false) }
    var moveMenu by remember { mutableStateOf(false) }
    var failure by remember { mutableStateOf<Throwable?>(null) }
    if (item == null) {
        LaunchedEffect(Unit) { onBack() }
        return
    }
    Page(
        title = item.titleText(),
        onBack = onBack,
        actions = { if (!item.trashed && writable) TextButton(onClick = onEdit) { Text(stringResource(R.string.edit)) } },
    ) {
        failure?.let { Banner(errorText(it), danger = true) }
        if (vault != null && vault.shared) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                VaultChip(vault)
                Spacer(Modifier.width(8.dp))
                if (vault.readOnly) {
                    Text(stringResource(R.string.read_only), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        }
        val order = listOf(
            "username", "password", "url", "totp", "cardholder", "card_number", "exp_month", "exp_year", "cvv", "pin", "notes",
        )
        val fields = item.fields
            .filter { it.name != "title" && it.value.isNotEmpty() }
            .sortedBy { f -> order.indexOf(f.name).let { if (it < 0) order.size else it } }
        for (f in fields) {
            when {
                f.name == "totp" -> TotpRow(f.value)
                f.name == "card_number" -> FieldRow(stringResource(R.string.card_number), f.value, concealed = true, display = cardInfoSafe(f.value))
                else -> FieldRow(labelFor(f.name), f.value, concealed = f.concealed, ltr = f.name in ltrFields || f.name.startsWith("url."), display = if (f.name == "url" || f.name.startsWith("url.")) shortOrFull(f.value) else null)
            }
        }
        Spacer(Modifier.size(8.dp))
        if (!item.trashed && writable && targets.isNotEmpty()) {
            Column {
                OutlinedButton(onClick = { moveMenu = true }, Modifier.fillMaxWidth()) { Text(stringResource(R.string.move_to)) }
                DropdownMenu(moveMenu, { moveMenu = false }) {
                    for (t in targets) {
                        DropdownMenuItem(
                            text = { Text(if (t.shared) vaultName(t.name) else stringResource(R.string.personal_vault)) },
                            onClick = {
                                moveMenu = false
                                scope.launch {
                                    failure = null
                                    try {
                                        app.vault.move(id, t.id)
                                        onBack()
                                    } catch (e: Throwable) {
                                        failure = e
                                    }
                                }
                            },
                        )
                    }
                }
            }
        }
        // Readers can't delete or restore.
        if (writable && item.trashed) {
            OutlinedButton(onClick = { scope.launch { app.vault.trash(id, false) } }, Modifier.fillMaxWidth()) {
                Text(stringResource(R.string.restore))
            }
        } else if (writable) {
            OutlinedButton(onClick = { confirmTrash = true }, Modifier.fillMaxWidth()) {
                Text(stringResource(R.string.delete), color = MaterialTheme.colorScheme.error)
            }
        }
    }
    if (confirmTrash) {
        AlertDialog(
            onDismissRequest = { confirmTrash = false },
            text = { Text(stringResource(R.string.delete_confirm)) },
            confirmButton = {
                TextButton(onClick = {
                    confirmTrash = false
                    scope.launch {
                        app.vault.trash(id, true)
                        onBack()
                    }
                }) { Text(stringResource(R.string.delete)) }
            },
            dismissButton = { TextButton(onClick = { confirmTrash = false }) { Text(stringResource(R.string.cancel)) } },
        )
    }
}

private fun cardInfoSafe(number: String): String? = runCatching { cardInfo(number) }.getOrNull()?.let { info ->
    info.digits.chunked(4).joinToString(" ")
}

private fun shortOrFull(url: String): String? = androidAppPackage(url)

@Composable
fun labelFor(name: String): String = when (name) {
    "username" -> stringResource(R.string.username)
    "password" -> stringResource(R.string.password)
    "url" -> stringResource(R.string.website)
    "totp" -> stringResource(R.string.one_time_code)
    "notes" -> stringResource(R.string.notes)
    "cardholder" -> stringResource(R.string.cardholder)
    "card_number" -> stringResource(R.string.card_number)
    "exp_month" -> stringResource(R.string.exp_month)
    "exp_year" -> stringResource(R.string.exp_year)
    "cvv" -> stringResource(R.string.cvv)
    "pin" -> stringResource(R.string.pin)
    else -> if (name.startsWith("url.")) stringResource(R.string.website) else name.removePrefix("custom:")
}

@Composable
fun FieldRow(label: String, value: String, concealed: Boolean, ltr: Boolean = true, display: String? = null) {
    val ctx = LocalContext.current
    var shown by remember { mutableStateOf(false) }
    val copied = stringResource(R.string.copied)
    var note by remember { mutableStateOf<String?>(null) }
    Surface(shape = RoundedCornerShape(12.dp), color = MaterialTheme.colorScheme.surface, modifier = Modifier.fillMaxWidth()) {
        Row(Modifier.padding(start = 14.dp, top = 10.dp, bottom = 10.dp, end = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(label, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                val text = if (concealed && !shown) "••••••••" else (display ?: value)
                val dir = if (ltr) LayoutDirection.Ltr else LocalLayoutDirection.current
                androidx.compose.runtime.CompositionLocalProvider(LocalLayoutDirection provides dir) {
                    Text(text, style = if (concealed) SecretText else MaterialTheme.typography.bodyLarge)
                }
                note?.let { Text(it, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.primary) }
            }
            if (concealed) {
                IconButton(onClick = { shown = !shown }) {
                    Icon(if (shown) Icons.Default.VisibilityOff else Icons.Default.Visibility, stringResource(if (shown) R.string.hide else R.string.show))
                }
            }
            IconButton(onClick = {
                Clipboard.copy(ctx, value, sensitive = concealed)
                note = copied
            }) { Icon(Icons.Default.ContentCopy, stringResource(R.string.copy)) }
        }
    }
}

@Composable
private fun TotpRow(secret: String) {
    var now by remember { mutableStateOf(System.currentTimeMillis()) }
    LaunchedEffect(secret) {
        while (true) {
            delay(1000)
            now = System.currentTimeMillis()
        }
    }
    val code = remember(now / 1000, secret) { runCatching { totpNow(secret) }.getOrNull() }
    if (code == null) {
        FieldRow(stringResource(R.string.totp), secret, concealed = true)
        return
    }
    Column {
        FieldRow(
            stringResource(R.string.one_time_code) + " · " + stringResource(R.string.seconds_left, code.secondsRemaining.toInt()),
            code.code,
            concealed = false,
            display = code.code.chunked(3).joinToString(" "),
        )
        LinearProgressIndicator(
            progress = { code.secondsRemaining.toFloat() / code.period.toFloat() },
            modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp),
        )
    }
}

// ---------------------------------------------------------------------------
// Edit / create
// ---------------------------------------------------------------------------

@Composable
fun ItemEditScreen(id: String?, kind: ItemKind, onBack: () -> Unit, onSaved: (String) -> Unit, onGenerate: ((String) -> Unit) -> Unit) {
    val app = LocalContext.current.amanfold
    val scope = rememberCoroutineScope()
    val existing = remember(id) { id?.let { app.vault.item(it) } }
    val k = existing?.kind ?: kind
    val names = when (k) {
        ItemKind.LOGIN -> listOf("title", "username", "password", "url", "totp", "notes")
        ItemKind.CARD -> listOf("title", "cardholder", "card_number", "exp_month", "exp_year", "cvv", "pin", "notes")
        else -> listOf("title", "notes")
    }
    val initial = names.associateWith { existing?.value(it) ?: "" }
    var values by rememberSaveable { mutableStateOf(initial) }
    var busy by remember { mutableStateOf(false) }
    var failure by remember { mutableStateOf<Throwable?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    val choices = remember { if (existing == null) app.vault.vaults().filter { !it.readOnly } else emptyList() }
    var target by remember { mutableStateOf(choices.firstOrNull { !it.shared }) }
    var vaultMenu by remember { mutableStateOf(false) }
    val badCard = stringResource(R.string.card_invalid)
    val badTotp = stringResource(R.string.totp_invalid)

    fun set(n: String, v: String) { values = values + (n to v) }

    val title = when (k) {
        ItemKind.LOGIN -> R.string.new_login
        ItemKind.CARD -> R.string.new_card
        else -> R.string.new_note
    }
    Page(stringResource(if (existing != null) R.string.edit else title), onBack = onBack) {
        if (choices.size > 1) {
            Column {
                val t = target
                OutlinedButton(onClick = { vaultMenu = true }, Modifier.fillMaxWidth()) {
                    Text(stringResource(R.string.save_in, if (t == null || !t.shared) stringResource(R.string.personal_vault) else vaultName(t.name)))
                }
                DropdownMenu(vaultMenu, { vaultMenu = false }) {
                    for (c in choices) {
                        DropdownMenuItem(
                            text = { Text(if (c.shared) vaultName(c.name) else stringResource(R.string.personal_vault)) },
                            onClick = { target = c; vaultMenu = false },
                        )
                    }
                }
            }
        }
        for (n in names) {
            val v = values[n] ?: ""
            when (n) {
                "password" -> SecretField(v, { set(n, it) }, labelFor(n), extra = {
                    TextButton(onClick = { onGenerate { set("password", it) } }) { Text(stringResource(R.string.generate)) }
                })
                "cvv", "pin" -> SecretField(v, { set(n, it.filter(Char::isDigit).take(if (n == "cvv") 4 else 12)) }, labelFor(n), keyboard = KeyboardType.NumberPassword)
                "totp" -> SecretField(v, { set(n, it) }, stringResource(R.string.totp), supporting = stringResource(R.string.totp_hint), keyboard = KeyboardType.Ascii)
                "card_number" -> Field(v, { set(n, it) }, labelFor(n), keyboard = KeyboardType.Number, style = SecretText)
                "exp_month" -> Field(v, { set(n, it.filter(Char::isDigit).take(2)) }, labelFor(n), keyboard = KeyboardType.Number)
                "exp_year" -> Field(v, { set(n, it.filter(Char::isDigit).take(4)) }, labelFor(n), keyboard = KeyboardType.Number)
                "url" -> Field(v, { set(n, it.trim()) }, labelFor(n), keyboard = KeyboardType.Uri)
                "username" -> Field(v, { set(n, it) }, labelFor(n), keyboard = KeyboardType.Email)
                "notes" -> Field(v, { set(n, it) }, labelFor(n), singleLine = false, minLines = 3)
                else -> Field(v, { set(n, it) }, if (n == "title") stringResource(R.string.title) else labelFor(n))
            }
        }
        error?.let { Banner(it, danger = true) }
        failure?.let { Banner(errorText(it), danger = true) }
        BusyButton(stringResource(R.string.save), busy) {
            val card = values["card_number"]?.takeIf { it.isNotBlank() }
            error = when {
                card != null && runCatching { cardInfo(card).luhnValid }.getOrDefault(false).not() -> badCard
                values["totp"]?.isNotBlank() == true && runCatching { totpNow(values["totp"]!!) }.isFailure -> badTotp
                else -> null
            }
            if (error != null) return@BusyButton
            busy = true
            failure = null
            scope.launch {
                try {
                    val set = values.filter { (n, v) -> v.isNotEmpty() && v != (initial[n] ?: "") }.map { (n, v) -> FieldInput(n, v) }
                    val remove = values.filter { (n, v) -> v.isEmpty() && (initial[n] ?: "").isNotEmpty() }.keys.toList()
                    val saved = if (existing == null) app.vault.create(k, set, target?.id) else {
                        app.vault.update(existing.id, set, remove)
                        existing.id
                    }
                    onSaved(saved)
                } catch (e: Throwable) {
                    failure = e
                } finally {
                    busy = false
                }
            }
        }
    }
}
