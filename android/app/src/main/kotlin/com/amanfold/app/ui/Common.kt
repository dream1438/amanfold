package com.amanfold.app.ui

import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Visibility
import androidx.compose.material.icons.filled.VisibilityOff
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.dp
import com.amanfold.app.R
import com.amanfold.client.Problem
import com.amanfold.client.problemOf

/** One sentence for any failure. */
@Composable
fun errorText(e: Throwable, hadCode: Boolean = false): String = when (problemOf(e, hadCode)) {
    Problem.WrongPassword -> stringResource(R.string.err_wrong_password)
    Problem.SecretKey -> stringResource(R.string.err_secret_key)
    Problem.RecoveryWords -> stringResource(R.string.err_recovery_words)
    Problem.Offline -> stringResource(R.string.err_offline)
    Problem.Exists -> stringResource(R.string.err_exists)
    Problem.BadServer -> stringResource(R.string.err_bad_server)
    Problem.ServerKey -> stringResource(R.string.err_server_key)
    Problem.Rollback -> stringResource(R.string.err_rollback)
    Problem.SignedOut -> stringResource(R.string.err_signed_out)
    Problem.Verification -> stringResource(R.string.err_verification)
    Problem.Code -> stringResource(R.string.err_code)
    Problem.RateLimited -> stringResource(R.string.err_rate_limited)
    Problem.Locked -> stringResource(R.string.err_locked)
    Problem.ReadOnly -> stringResource(R.string.err_read_only)
    Problem.SafetyNumber -> stringResource(R.string.err_safety_number)
    Problem.Unverified -> stringResource(R.string.err_unverified)
    Problem.NotFound -> stringResource(R.string.err_not_found)
    Problem.Server -> stringResource(R.string.err_server, e.message ?: "")
    Problem.Other -> stringResource(R.string.err_generic, e.message ?: e.javaClass.simpleName)
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun Page(
    title: String,
    onBack: (() -> Unit)? = null,
    actions: @Composable () -> Unit = {},
    floating: @Composable () -> Unit = {},
    scroll: Boolean = true,
    content: @Composable ColumnScope.() -> Unit,
) {
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(title, maxLines = 1) },
                navigationIcon = {
                    if (onBack != null) {
                        IconButton(onClick = onBack) {
                            Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.back))
                        }
                    }
                },
                actions = { actions() },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.background),
            )
        },
        floatingActionButton = floating,
        containerColor = MaterialTheme.colorScheme.background,
    ) { pad ->
        val m = Modifier
            .padding(pad)
            .fillMaxSize()
            .imePadding()
        Column(
            modifier = if (scroll) m.verticalScroll(rememberScrollState()).padding(horizontal = 20.dp, vertical = 8.dp) else m,
            verticalArrangement = Arrangement.spacedBy(12.dp),
            content = content,
        )
    }
}

@Composable
fun Logo(modifier: Modifier = Modifier) {
    Row(modifier, verticalAlignment = Alignment.CenterVertically) {
        Image(painterResource(R.drawable.ic_mark), contentDescription = null, modifier = Modifier.size(40.dp))
        Spacer(Modifier.width(10.dp))
        // The English logo everywhere for now, also on Arabic screens.
        Text("Amanfold", style = MaterialTheme.typography.headlineMedium, color = MaterialTheme.colorScheme.onBackground)
        Spacer(Modifier.width(8.dp))
        BetaBadge()
    }
}

@Composable
fun BetaBadge() {
    Box(
        Modifier
            .background(MaterialTheme.colorScheme.primaryContainer, RoundedCornerShape(6.dp))
            .padding(horizontal = 6.dp, vertical = 2.dp),
    ) {
        Text(stringResource(R.string.beta), style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onPrimaryContainer)
    }
}

@Composable
fun Banner(text: String, danger: Boolean = false) {
    Surface(
        color = if (danger) MaterialTheme.colorScheme.errorContainer else MaterialTheme.colorScheme.secondaryContainer,
        contentColor = if (danger) MaterialTheme.colorScheme.onErrorContainer else MaterialTheme.colorScheme.onSecondaryContainer,
        shape = RoundedCornerShape(12.dp),
        modifier = Modifier
            .fillMaxWidth()
            .semantics { liveRegion = LiveRegionMode.Polite },
    ) {
        Text(text, Modifier.padding(12.dp), style = MaterialTheme.typography.bodyMedium)
    }
}

@Composable
fun Field(
    value: String,
    onChange: (String) -> Unit,
    label: String,
    modifier: Modifier = Modifier,
    keyboard: KeyboardType = KeyboardType.Text,
    supporting: String? = null,
    isError: Boolean = false,
    singleLine: Boolean = true,
    minLines: Int = 1,
    style: TextStyle? = null,
    trailing: @Composable (() -> Unit)? = null,
) {
    OutlinedTextField(
        value = value,
        onValueChange = onChange,
        label = { Text(label) },
        modifier = modifier.fillMaxWidth(),
        singleLine = singleLine,
        minLines = minLines,
        isError = isError,
        textStyle = style ?: MaterialTheme.typography.bodyLarge,
        supportingText = supporting?.let { { Text(it) } },
        keyboardOptions = KeyboardOptions(keyboardType = keyboard, imeAction = if (singleLine) ImeAction.Next else ImeAction.Default),
        trailingIcon = trailing,
    )
}

/** A secret input: hidden until shown, never suggested by the keyboard. */
@Composable
fun SecretField(
    value: String,
    onChange: (String) -> Unit,
    label: String,
    modifier: Modifier = Modifier,
    supporting: String? = null,
    isError: Boolean = false,
    keyboard: KeyboardType = KeyboardType.Password,
    extra: @Composable (() -> Unit)? = null,
) {
    var shown by rememberSaveable { mutableStateOf(false) }
    OutlinedTextField(
        value = value,
        onValueChange = onChange,
        label = { Text(label) },
        modifier = modifier.fillMaxWidth(),
        singleLine = true,
        isError = isError,
        textStyle = SecretText,
        supportingText = supporting?.let { { Text(it) } },
        visualTransformation = if (shown) VisualTransformation.None else PasswordVisualTransformation(),
        keyboardOptions = KeyboardOptions(keyboardType = keyboard, autoCorrectEnabled = false, imeAction = ImeAction.Next),
        trailingIcon = {
            Row {
                extra?.invoke()
                IconButton(onClick = { shown = !shown }) {
                    Icon(
                        if (shown) Icons.Default.VisibilityOff else Icons.Default.Visibility,
                        contentDescription = stringResource(if (shown) R.string.hide else R.string.show),
                    )
                }
            }
        },
    )
}

@Composable
fun BusyButton(text: String, busy: Boolean, enabled: Boolean = true, onClick: () -> Unit) {
    Button(onClick = onClick, enabled = enabled && !busy, modifier = Modifier.fillMaxWidth().height(52.dp)) {
        if (busy) {
            CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp)
            Spacer(Modifier.width(12.dp))
            Text(stringResource(R.string.working))
        } else {
            Text(text)
        }
    }
}

@Composable
fun Centered(content: @Composable ColumnScope.() -> Unit) {
    Column(
        Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background)
            .verticalScroll(remember { androidx.compose.foundation.ScrollState(0) })
            .imePadding()
            .padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp, Alignment.CenterVertically),
        horizontalAlignment = Alignment.Start,
        content = content,
    )
}
