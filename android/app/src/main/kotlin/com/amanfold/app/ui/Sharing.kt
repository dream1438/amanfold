package com.amanfold.app.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Groups
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.Verified
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role as A11yRole
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import com.amanfold.app.R
import com.amanfold.app.amanfold
import com.amanfold.client.InviteView
import com.amanfold.client.MemberView
import com.amanfold.client.MembersView
import com.amanfold.client.Role
import com.amanfold.client.SharingView
import com.amanfold.client.VaultKind
import com.amanfold.client.WaitingView
import kotlinx.coroutines.launch

// ---------------------------------------------------------------------------
// Labels
// ---------------------------------------------------------------------------

@Composable
fun roleText(r: Role): String = stringResource(
    when (r) {
        Role.Owner -> R.string.role_owner
        Role.Manager -> R.string.role_manager
        Role.Writer -> R.string.role_writer
        Role.Reader -> R.string.role_reader
    },
)

@Composable
fun roleHint(r: Role): String = stringResource(
    when (r) {
        Role.Owner -> R.string.role_owner_hint
        Role.Manager -> R.string.role_manager_hint
        Role.Writer -> R.string.role_writer_hint
        Role.Reader -> R.string.role_reader_hint
    },
)

/** A shared vault's name; members never label it. */
@Composable
fun vaultName(name: String): String = name.ifBlank { stringResource(R.string.shared_vault) }

fun VaultKind.icon(): ImageVector = if (this == VaultKind.Team) Icons.Default.Groups else Icons.Default.Home

/** A safety number: Latin digits, always left to right. */
@Composable
fun SafetyNumber(value: String) {
    Surface(shape = RoundedCornerShape(10.dp), color = MaterialTheme.colorScheme.surfaceVariant, modifier = Modifier.fillMaxWidth()) {
        CompositionLocalProvider(LocalLayoutDirection provides LayoutDirection.Ltr) {
            Text(
                value,
                Modifier.padding(14.dp),
                style = MaterialTheme.typography.titleMedium.copy(fontFamily = FontFamily.Monospace),
            )
        }
    }
}

@Composable
private fun SectionTitle(text: String) {
    Text(text, style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.primary, modifier = Modifier.padding(top = 8.dp))
}

@Composable
private fun Card(onClick: (() -> Unit)? = null, content: @Composable () -> Unit) {
    Surface(
        shape = RoundedCornerShape(12.dp),
        color = MaterialTheme.colorScheme.surface,
        modifier = Modifier.fillMaxWidth().let { if (onClick != null) it.clickable(onClick = onClick) else it },
    ) { Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) { content() } }
}

/**
 * Asks the person to compare a safety number with the other person, in
 * person or on a call, before trusting them.
 */
@Composable
private fun CompareDialog(
    email: String,
    fingerprint: String,
    confirm: String,
    onConfirm: () -> Unit,
    onDismiss: () -> Unit,
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.compare_title)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Text(stringResource(R.string.compare_body, email))
                SafetyNumber(fingerprint)
                Text(stringResource(R.string.compare_warning), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        },
        confirmButton = { TextButton(onClick = onConfirm) { Text(confirm) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.they_dont_match)) } },
    )
}

// ---------------------------------------------------------------------------
// Sharing overview
// ---------------------------------------------------------------------------

@Composable
fun SharingScreen(onBack: () -> Unit, onOpenVault: (String) -> Unit) {
    val app = LocalContext.current.amanfold
    val scope = rememberCoroutineScope()
    var reload by remember { mutableIntStateOf(0) }
    var view by remember { mutableStateOf<SharingView?>(null) }
    var failure by remember { mutableStateOf<Throwable?>(null) }
    var notice by remember { mutableStateOf<String?>(null) }
    var accepting by remember { mutableStateOf<InviteView?>(null) }
    var declining by remember { mutableStateOf<InviteView?>(null) }
    var checking by remember { mutableStateOf<WaitingView?>(null) }
    var creating by remember { mutableStateOf(false) }
    val acceptedNote = stringResource(R.string.invite_accepted)

    LaunchedEffect(reload) {
        failure = null
        try {
            runCatching { app.vault.sync() }
            view = app.vault.sharing()
        } catch (e: Throwable) {
            failure = e
        }
    }

    fun act(block: suspend () -> Unit) {
        scope.launch {
            failure = null
            try {
                block()
            } catch (e: Throwable) {
                failure = e
            }
            reload++
        }
    }

    Page(
        title = stringResource(R.string.sharing),
        onBack = onBack,
        actions = { TextButton(onClick = { creating = true }) { Text(stringResource(R.string.new_vault)) } },
    ) {
        failure?.let { Banner(errorText(it), danger = true) }
        notice?.let { Banner(it) }
        val v = view
        if (v == null) {
            if (failure == null) CircularProgressIndicator(Modifier.align(Alignment.CenterHorizontally))
            return@Page
        }

        if (v.invites.isNotEmpty()) {
            SectionTitle(stringResource(R.string.invitations))
            for (inv in v.invites) {
                Card {
                    Text(stringResource(R.string.invite_from, inv.fromEmail), style = MaterialTheme.typography.titleMedium)
                    Text(stringResource(R.string.invite_role, roleText(inv.role)), style = MaterialTheme.typography.bodyMedium)
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        Button(onClick = { accepting = inv }) { Text(stringResource(R.string.compare_and_accept)) }
                        TextButton(onClick = { declining = inv }) { Text(stringResource(R.string.decline)) }
                    }
                }
            }
        }

        if (v.waiting.isNotEmpty()) {
            SectionTitle(stringResource(R.string.waiting_title))
            for (w in v.waiting) {
                Card {
                    Text(stringResource(R.string.waiting_body, w.email))
                    Button(onClick = { checking = w }) { Text(stringResource(R.string.compare_numbers)) }
                }
            }
        }

        SectionTitle(stringResource(R.string.shared_vaults))
        if (v.vaults.isEmpty()) {
            Text(stringResource(R.string.no_shared_vaults), color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        for (sv in v.vaults) {
            Card(onClick = { onOpenVault(sv.id) }) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(sv.kind.icon(), null, tint = MaterialTheme.colorScheme.primary)
                    Spacer(Modifier.width(12.dp))
                    Column(Modifier.weight(1f)) {
                        Text(vaultName(sv.name), style = MaterialTheme.typography.titleMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        Text(
                            stringResource(R.string.vault_summary, roleText(sv.role), sv.items),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
                if (sv.needsVerification) Banner(stringResource(R.string.needs_verification), danger = true)
            }
        }

        SectionTitle(stringResource(R.string.your_safety_number))
        Text(stringResource(R.string.your_safety_number_hint), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        SafetyNumber(v.myFingerprint)
    }

    accepting?.let { inv ->
        CompareDialog(
            email = inv.fromEmail,
            fingerprint = inv.fromFingerprint,
            confirm = stringResource(R.string.they_match_accept),
            onConfirm = {
                accepting = null
                act {
                    app.vault.acceptInvite(inv.inviteId, inv.fromFingerprint)
                    notice = acceptedNote
                }
            },
            onDismiss = { accepting = null },
        )
    }
    declining?.let { inv ->
        AlertDialog(
            onDismissRequest = { declining = null },
            text = { Text(stringResource(R.string.decline_confirm, inv.fromEmail)) },
            confirmButton = {
                TextButton(onClick = {
                    declining = null
                    act { app.vault.cancelInvite(inv.inviteId) }
                }) { Text(stringResource(R.string.decline)) }
            },
            dismissButton = { TextButton(onClick = { declining = null }) { Text(stringResource(R.string.cancel)) } },
        )
    }
    checking?.let { w ->
        CompareDialog(
            email = w.email,
            fingerprint = w.fingerprint,
            confirm = stringResource(R.string.they_match),
            onConfirm = {
                checking = null
                act { app.vault.verify(w.vaultId, w.accountId, w.fingerprint) }
            },
            onDismiss = { checking = null },
        )
    }
    if (creating) {
        CreateVaultDialog(
            onCreate = { name, kind ->
                creating = false
                act { onOpenVault(app.vault.createVault(name, kind)) }
            },
            onDismiss = { creating = false },
        )
    }
}

@Composable
private fun CreateVaultDialog(onCreate: (String, VaultKind) -> Unit, onDismiss: () -> Unit) {
    var name by remember { mutableStateOf("") }
    var kind by remember { mutableStateOf(VaultKind.Family) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.new_vault)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Field(name, { name = it.take(60) }, stringResource(R.string.vault_name))
                for ((k, label) in listOf(VaultKind.Family to R.string.kind_family, VaultKind.Team to R.string.kind_team)) {
                    Row(
                        Modifier.fillMaxWidth().selectable(selected = kind == k, onClick = { kind = k }, role = A11yRole.RadioButton),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        RadioButton(selected = kind == k, onClick = null)
                        Spacer(Modifier.width(8.dp))
                        Text(stringResource(label))
                    }
                }
                Text(stringResource(R.string.new_vault_hint), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        },
        confirmButton = { TextButton(onClick = { onCreate(name, kind) }) { Text(stringResource(R.string.create)) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
    )
}

// ---------------------------------------------------------------------------
// One shared vault: members, invitations, leaving
// ---------------------------------------------------------------------------

@Composable
fun MembersScreen(vaultId: String, onBack: () -> Unit) {
    val app = LocalContext.current.amanfold
    val scope = rememberCoroutineScope()
    val revision by app.vault.revision.collectAsState()
    var reload by remember { mutableIntStateOf(0) }
    var view by remember { mutableStateOf<MembersView?>(null) }
    var failure by remember { mutableStateOf<Throwable?>(null) }
    var busy by remember { mutableStateOf(false) }
    var selected by remember { mutableStateOf<MemberView?>(null) }
    var verifying by remember { mutableStateOf<MemberView?>(null) }
    var removing by remember { mutableStateOf<MemberView?>(null) }
    var leaving by remember { mutableStateOf(false) }
    var email by remember { mutableStateOf("") }
    var inviteRole by remember { mutableStateOf(Role.Writer) }
    var roleMenu by remember { mutableStateOf(false) }
    var sentNote by remember { mutableStateOf<String?>(null) }

    val tag = remember(revision, vaultId) { if (app.vault.isUnlocked) app.vault.vaults().firstOrNull { it.id == vaultId } else null }
    if (tag == null) {
        LaunchedEffect(Unit) { onBack() }
        return
    }
    val needsVerification = remember { mutableStateOf(false) }

    LaunchedEffect(reload) {
        failure = null
        try {
            view = app.vault.members(vaultId)
            needsVerification.value = app.vault.sharing().vaults.firstOrNull { it.id == vaultId }?.needsVerification == true
        } catch (e: Throwable) {
            failure = e
        }
    }

    fun act(after: (() -> Unit)? = null, block: suspend () -> Unit) {
        busy = true
        scope.launch {
            failure = null
            try {
                block()
                after?.invoke()
            } catch (e: Throwable) {
                failure = e
            } finally {
                busy = false
            }
            reload++
        }
    }

    val manage = tag.role.canManage
    val sentText = stringResource(R.string.invite_sent)
    Page(title = vaultName(tag.name), onBack = onBack) {
        failure?.let { Banner(errorText(it), danger = true) }
        if (needsVerification.value) Banner(stringResource(R.string.needs_verification), danger = true)
        Text(stringResource(R.string.your_role, roleText(tag.role)), style = MaterialTheme.typography.bodyMedium)
        Text(roleHint(tag.role), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)

        val v = view
        if (v == null) {
            if (failure == null) CircularProgressIndicator(Modifier.align(Alignment.CenterHorizontally))
        } else {
            SectionTitle(stringResource(R.string.members))
            for (m in v.members.sortedWith(compareBy({ !it.me }, { it.role.ordinal }, { it.email }))) {
                Card(onClick = if (m.me) null else ({ selected = m })) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Column(Modifier.weight(1f)) {
                            Text(
                                if (m.me) stringResource(R.string.member_you, m.email) else m.email,
                                style = MaterialTheme.typography.titleMedium,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                            Text(roleText(m.role), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                        when {
                            m.me -> Unit
                            !m.active -> Text(stringResource(R.string.member_pending), color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.labelMedium)
                            m.verified -> Icon(Icons.Default.Verified, stringResource(R.string.member_verified), tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(20.dp))
                            else -> Text(stringResource(R.string.member_unverified), color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.labelMedium)
                        }
                    }
                    if (!m.me && !m.active && manage) {
                        Text(stringResource(R.string.pending_hint), style = MaterialTheme.typography.bodySmall)
                    }
                }
            }

            if (manage) {
                SectionTitle(stringResource(R.string.invite_someone))
                Field(email, { email = it.trim() }, stringResource(R.string.email), keyboard = KeyboardType.Email)
                Column {
                    OutlinedButton(onClick = { roleMenu = true }, Modifier.fillMaxWidth()) {
                        Text(stringResource(R.string.invite_as, roleText(inviteRole)))
                    }
                    DropdownMenu(roleMenu, { roleMenu = false }) {
                        val choices = if (tag.role == Role.Owner) listOf(Role.Manager, Role.Writer, Role.Reader) else listOf(Role.Writer, Role.Reader)
                        for (r in choices) {
                            DropdownMenuItem(
                                text = {
                                    Column {
                                        Text(roleText(r))
                                        Text(roleHint(r), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                                    }
                                },
                                onClick = { inviteRole = r; roleMenu = false },
                            )
                        }
                    }
                }
                sentNote?.let { Banner(it) }
                BusyButton(stringResource(R.string.send_invite), busy, enabled = email.contains('@')) {
                    val to = email
                    act(after = { email = ""; sentNote = sentText }) { app.vault.invite(vaultId, to, inviteRole) }
                }
                Text(stringResource(R.string.invite_hint), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                if (v.invites.isNotEmpty()) {
                    SectionTitle(stringResource(R.string.sent_invites))
                    for (inv in v.invites) {
                        Card {
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Column(Modifier.weight(1f)) {
                                    Text(inv.email, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                    Text(roleText(inv.role), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                                }
                                TextButton(onClick = { act { app.vault.cancelInvite(inv.inviteId) } }) { Text(stringResource(R.string.cancel_invite)) }
                            }
                        }
                    }
                }
            }
        }

        Spacer(Modifier.size(8.dp))
        OutlinedButton(onClick = { leaving = true }, Modifier.fillMaxWidth()) {
            Text(
                stringResource(if (tag.role == Role.Owner) R.string.delete_vault else R.string.leave_vault),
                color = MaterialTheme.colorScheme.error,
            )
        }
    }

    selected?.let { m ->
        MemberDialog(
            member = m,
            myRole = tag.role,
            onVerify = { selected = null; verifying = m },
            onRole = { r -> selected = null; act { app.vault.setRole(vaultId, m.accountId, r) } },
            onRemove = { selected = null; removing = m },
            onDismiss = { selected = null },
        )
    }
    verifying?.let { m ->
        CompareDialog(
            email = m.email,
            fingerprint = m.fingerprint,
            confirm = stringResource(if (!m.active && manage) R.string.they_match_give_access else R.string.they_match),
            onConfirm = { verifying = null; act { app.vault.verify(vaultId, m.accountId, m.fingerprint) } },
            onDismiss = { verifying = null },
        )
    }
    removing?.let { m ->
        AlertDialog(
            onDismissRequest = { removing = null },
            title = { Text(stringResource(R.string.remove_member_title, m.email)) },
            text = { Text(stringResource(R.string.remove_member_body)) },
            confirmButton = {
                TextButton(onClick = { removing = null; act { app.vault.removeMember(vaultId, m.accountId) } }) {
                    Text(stringResource(R.string.remove), color = MaterialTheme.colorScheme.error)
                }
            },
            dismissButton = { TextButton(onClick = { removing = null }) { Text(stringResource(R.string.cancel)) } },
        )
    }
    if (leaving) {
        val owner = tag.role == Role.Owner
        AlertDialog(
            onDismissRequest = { leaving = false },
            text = { Text(stringResource(if (owner) R.string.delete_vault_confirm else R.string.leave_vault_confirm, vaultName(tag.name))) },
            confirmButton = {
                TextButton(onClick = {
                    leaving = false
                    act(after = onBack) { if (owner) app.vault.deleteVault(vaultId) else app.vault.leaveVault(vaultId) }
                }) {
                    Text(stringResource(if (owner) R.string.delete_vault else R.string.leave_vault), color = MaterialTheme.colorScheme.error)
                }
            },
            dismissButton = { TextButton(onClick = { leaving = false }) { Text(stringResource(R.string.cancel)) } },
        )
    }
}

@Composable
private fun MemberDialog(
    member: MemberView,
    myRole: Role,
    onVerify: () -> Unit,
    onRole: (Role) -> Unit,
    onRemove: () -> Unit,
    onDismiss: () -> Unit,
) {
    // Owners change any role but their own; managers manage writers and readers.
    val canChange = member.role != Role.Owner && (myRole == Role.Owner || (myRole == Role.Manager && member.role != Role.Manager))
    val roles = if (myRole == Role.Owner) listOf(Role.Manager, Role.Writer, Role.Reader) else listOf(Role.Writer, Role.Reader)
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(member.email, maxLines = 2, overflow = TextOverflow.Ellipsis) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Text(roleText(member.role) + " · " + roleHint(member.role), style = MaterialTheme.typography.bodySmall)
                Text(stringResource(R.string.safety_number))
                SafetyNumber(member.fingerprint)
                if (!member.verified) Button(onClick = onVerify, Modifier.fillMaxWidth()) { Text(stringResource(R.string.compare_numbers)) }
                if (canChange && member.active) {
                    Text(stringResource(R.string.change_role), style = MaterialTheme.typography.titleSmall)
                    for (r in roles) {
                        Row(
                            Modifier.fillMaxWidth().selectable(selected = member.role == r, onClick = { if (member.role != r) onRole(r) }, role = A11yRole.RadioButton),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            RadioButton(selected = member.role == r, onClick = null)
                            Spacer(Modifier.width(8.dp))
                            Text(roleText(r))
                        }
                    }
                }
                if (canChange) {
                    OutlinedButton(onClick = onRemove, Modifier.fillMaxWidth()) {
                        Text(stringResource(R.string.remove), color = MaterialTheme.colorScheme.error)
                    }
                }
            }
        },
        confirmButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.done)) } },
    )
}
