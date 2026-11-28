package com.amanfold.app.ui

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp

// Brand palette (brand/tokens/tokens.json). Amber is the one highlight per
// screen; nothing else uses it.
object Brand {
    val Green = Color(0xFF0B6E4F)
    val Green700 = Color(0xFF09593F)
    val Green400 = Color(0xFF2CB183)
    val Green300 = Color(0xFF5FCFA3)
    val Green50 = Color(0xFFEAF7F1)
    val Mint = Color(0xFF9BE4C4)
    val Ink = Color(0xFF0C1F1A)
    val Paper = Color(0xFFF4F6F3)
    val Amber = Color(0xFFF5B544)
    val Danger = Color(0xFFB3261E)
    val DangerDark = Color(0xFFF2877B)
    val Muted = Color(0xFF55655F)
    val Border = Color(0xFFDDE3DE)
    val Night = Color(0xFF081410)
    val NightSurface = Color(0xFF0C1F1A)
    val NightBorder = Color(0xFF1F302A)
}

private val light = lightColorScheme(
    primary = Brand.Green,
    onPrimary = Color.White,
    primaryContainer = Brand.Green50,
    onPrimaryContainer = Brand.Green700,
    secondary = Brand.Green700,
    secondaryContainer = Color(0xFFCFEFE0),
    onSecondaryContainer = Brand.Ink,
    tertiary = Brand.Amber,
    onTertiary = Brand.Ink,
    background = Brand.Paper,
    onBackground = Brand.Ink,
    surface = Color.White,
    onSurface = Brand.Ink,
    surfaceVariant = Color(0xFFEAEEEA),
    onSurfaceVariant = Brand.Muted,
    surfaceContainer = Color.White,
    surfaceContainerHigh = Color(0xFFF8FAF8),
    outline = Color(0xFFC2CCC6),
    outlineVariant = Brand.Border,
    error = Brand.Danger,
    onError = Color.White,
    errorContainer = Color(0xFFFBE9E7),
    onErrorContainer = Color(0xFF7A1A14),
)

private val dark = darkColorScheme(
    primary = Brand.Green400,
    onPrimary = Brand.Ink,
    primaryContainer = Color(0xFF084632),
    onPrimaryContainer = Brand.Mint,
    secondary = Brand.Green300,
    secondaryContainer = Color(0xFF0A3326),
    onSecondaryContainer = Brand.Mint,
    tertiary = Color(0xFFF7C768),
    onTertiary = Brand.Ink,
    background = Brand.Night,
    onBackground = Color(0xFFEAEEEA),
    surface = Brand.NightSurface,
    onSurface = Color(0xFFEAEEEA),
    surfaceVariant = Color(0xFF1F302A),
    onSurfaceVariant = Color(0xFF93A29B),
    surfaceContainer = Brand.NightSurface,
    surfaceContainerHigh = Color(0xFF13261F),
    outline = Color(0xFF3A4A44),
    outlineVariant = Brand.NightBorder,
    error = Brand.DangerDark,
    onError = Brand.Ink,
    errorContainer = Color(0xFF4A1410),
    onErrorContainer = Color(0xFFFBD3CE),
)

/** Passwords, codes and the Secret Key: monospaced, so 0/O and 1/l differ. */
val SecretText = TextStyle(fontFamily = FontFamily.Monospace, fontSize = 16.sp, letterSpacing = 0.5.sp)

@Composable
fun AmanfoldTheme(content: @Composable () -> Unit) {
    val base = Typography()
    MaterialTheme(
        colorScheme = if (isSystemInDarkTheme()) dark else light,
        typography = base.copy(
            headlineSmall = base.headlineSmall.copy(fontWeight = FontWeight.SemiBold),
            titleLarge = base.titleLarge.copy(fontWeight = FontWeight.SemiBold),
        ),
        content = content,
    )
}
