package com.amanfold.app

import android.os.Bundle
import android.view.WindowManager
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.appcompat.app.AppCompatActivity
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import kotlinx.coroutines.launch
import androidx.compose.runtime.setValue
import com.amanfold.app.ui.AmanfoldTheme
import com.amanfold.app.ui.CreateAccountScreen
import com.amanfold.app.ui.GeneratorScreen
import com.amanfold.app.ui.ItemDetailScreen
import com.amanfold.app.ui.ItemEditScreen
import com.amanfold.app.ui.MembersScreen
import com.amanfold.app.ui.SharingScreen
import com.amanfold.app.ui.QuickUnlockOffer
import com.amanfold.app.ui.RecoverScreen
import com.amanfold.app.ui.RecoveryKitScreen
import com.amanfold.app.ui.SettingsScreen
import com.amanfold.app.ui.SignInScreen
import com.amanfold.app.ui.UnlockScreen
import com.amanfold.app.ui.VaultListScreen
import com.amanfold.app.ui.WelcomeScreen
import com.amanfold.client.RecoveryKit
import com.amanfold.client.VaultState
import com.amanfold.core.ItemKind

/** Screens, kept on a simple back stack. */
sealed interface Screen {
    data object Welcome : Screen
    data object Create : Screen
    data object SignIn : Screen
    data object Recover : Screen
    data object List : Screen
    data class Detail(val id: String) : Screen
    data class Edit(val id: String?, val kind: ItemKind) : Screen
    data class Generator(val forEdit: Boolean) : Screen
    data object Settings : Screen
    data object Sharing : Screen
    data class Members(val vaultId: String) : Screen
}

class MainActivity : AppCompatActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // No screenshots, screen recording or previews in the app switcher.
        window.setFlags(WindowManager.LayoutParams.FLAG_SECURE, WindowManager.LayoutParams.FLAG_SECURE)
        enableEdgeToEdge()
        setContent { AmanfoldTheme { AppRoot() } }
    }
}

@Composable
private fun AppRoot() {
    val ctx = androidx.compose.ui.platform.LocalContext.current
    val app = ctx.amanfold
    val state by app.vault.state.collectAsState()
    // The recovery kit lives only in memory, only until the person confirms they saved it.
    var kit by remember { mutableStateOf<RecoveryKit?>(null) }
    var offerQuick by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()

    kit?.let { k ->
        RecoveryKitScreen(k) {
            kit = null
            offerQuick = true
        }
        return
    }

    when (state) {
        VaultState.NoAccount -> Onboarding(onCreated = { kit = it })
        VaultState.Locked -> UnlockScreen(onSignOut = {
            app.quickKeys.clear()
            app.prefs.clearAccountState()
            scope.launch { app.vault.signout() }
        })
        VaultState.Unlocked -> {
            LaunchedEffect(Unit) {
                if (!app.prefs.quickUnlockOffered && app.quickKeys.available() && !app.vault.hasQuickUnlock) offerQuick = true
            }
            Vault()
            if (offerQuick && kit == null) {
                QuickUnlockOffer {
                    offerQuick = false
                    app.prefs.quickUnlockOffered = true
                }
            }
        }
    }
}

@Composable
private fun Onboarding(onCreated: (RecoveryKit) -> Unit) {
    val stack = remember { mutableStateListOf<Screen>(Screen.Welcome) }
    val top = stack.last()
    BackHandler(enabled = stack.size > 1) { stack.removeAt(stack.lastIndex) }
    val back = { if (stack.size > 1) stack.removeAt(stack.lastIndex) }
    when (top) {
        Screen.Create -> CreateAccountScreen(onBack = { back() }, onCreated = onCreated)
        Screen.SignIn -> SignInScreen(onBack = { back() }, onForgot = { stack.add(Screen.Recover) })
        Screen.Recover -> RecoverScreen(onBack = { back() })
        else -> WelcomeScreen(onCreate = { stack.add(Screen.Create) }, onSignIn = { stack.add(Screen.SignIn) })
    }
}

@Composable
private fun Vault() {
    val stack = remember { mutableStateListOf<Screen>(Screen.List) }
    // The edit screen waiting for a generated password.
    var pendingUse by remember { mutableStateOf<((String) -> Unit)?>(null) }
    val top = stack.last()
    val back = { if (stack.size > 1) stack.removeAt(stack.lastIndex) }
    BackHandler(enabled = stack.size > 1) { back() }
    when (top) {
        is Screen.Detail -> ItemDetailScreen(top.id, onBack = { back() }, onEdit = { stack.add(Screen.Edit(top.id, ItemKind.LOGIN)) })
        is Screen.Edit -> ItemEditScreen(
            top.id,
            top.kind,
            onBack = { back() },
            onSaved = { id ->
                back()
                if (top.id == null) stack.add(Screen.Detail(id))
            },
            onGenerate = { use ->
                pendingUse = use
                stack.add(Screen.Generator(forEdit = true))
            },
        )
        is Screen.Generator -> GeneratorScreen(
            onBack = { back() },
            onUse = if (top.forEdit) { v ->
                pendingUse?.invoke(v)
                pendingUse = null
                back()
            } else null,
        )
        Screen.Settings -> SettingsScreen(onBack = { back() })
        Screen.Sharing -> SharingScreen(onBack = { back() }, onOpenVault = { stack.add(Screen.Members(it)) })
        is Screen.Members -> MembersScreen(top.vaultId, onBack = { back() })
        else -> VaultListScreen(
            onOpen = { stack.add(Screen.Detail(it)) },
            onAdd = { stack.add(Screen.Edit(null, it)) },
            onSettings = { stack.add(Screen.Settings) },
            onGenerator = { stack.add(Screen.Generator(forEdit = false)) },
            onSharing = { stack.add(Screen.Sharing) },
        )
    }
}
