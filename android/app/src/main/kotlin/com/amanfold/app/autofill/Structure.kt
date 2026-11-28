package com.amanfold.app.autofill

import android.app.assist.AssistStructure
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.view.View
import android.view.autofill.AutofillId
import com.amanfold.client.autofill.FieldNode
import com.amanfold.core.androidAppUrl
import java.security.MessageDigest

/** A screen to fill: its fields in the client's terms, and their platform ids. */
class ParsedScreen(val packageName: String, val nodes: List<FieldNode>, val ids: List<AutofillId>)

/** Walks the view tree. Web documents pass their domain down to their fields. */
fun parseStructure(structure: AssistStructure): ParsedScreen {
    val nodes = ArrayList<FieldNode>()
    val ids = ArrayList<AutofillId>()

    fun visit(n: AssistStructure.ViewNode, domain: String?, scheme: String?, hidden: Boolean) {
        val d = n.webDomain?.takeIf { it.isNotBlank() } ?: domain
        val s = (if (Build.VERSION.SDK_INT >= 28) n.webScheme else null) ?: if (n.webDomain != null) null else scheme
        val gone = hidden || n.visibility != View.VISIBLE
        val id = n.autofillId
        if (id != null && n.autofillType == View.AUTOFILL_TYPE_TEXT && n.isEnabled) {
            val html = n.htmlInfo
            val attrs = if (html != null && html.tag.equals("input", ignoreCase = true)) {
                html.attributes?.associate { it.first.lowercase() to (it.second ?: "") } ?: emptyMap()
            } else {
                emptyMap()
            }
            nodes += FieldNode(
                index = ids.size,
                autofillHints = n.autofillHints?.toList() ?: emptyList(),
                inputType = n.inputType,
                htmlAttributes = attrs,
                idEntry = n.idEntry,
                hint = n.hint,
                label = n.contentDescription?.toString(),
                webDomain = d,
                webScheme = s,
                focused = n.isFocused,
                visible = !gone,
                text = n.autofillValue?.takeIf { it.isText }?.textValue?.toString(),
            )
            ids += id
        }
        for (i in 0 until n.childCount) visit(n.getChildAt(i), d, s, gone)
    }

    for (w in 0 until structure.windowNodeCount) {
        visit(structure.getWindowNodeAt(w).rootViewNode, null, null, false)
    }
    return ParsedScreen(structure.activityComponent.packageName, nodes, ids)
}

/**
 * The app's identity for logins: package name plus the SHA-256 of its
 * signing certificate. `null` when the app cannot be identified safely
 * (not visible to us, or signed by several keys).
 */
fun appIdentityUrl(context: Context, packageName: String): String? = runCatching {
    val pm = context.packageManager
    val info = if (Build.VERSION.SDK_INT >= 33) {
        pm.getPackageInfo(packageName, PackageManager.PackageInfoFlags.of(PackageManager.GET_SIGNING_CERTIFICATES.toLong()))
    } else {
        @Suppress("DEPRECATION")
        pm.getPackageInfo(packageName, PackageManager.GET_SIGNING_CERTIFICATES)
    }
    val signing = info.signingInfo ?: return null
    if (signing.hasMultipleSigners()) return null
    val cert = signing.apkContentsSigners?.singleOrNull() ?: return null
    androidAppUrl(packageName, MessageDigest.getInstance("SHA-256").digest(cert.toByteArray()))
}.getOrNull()

fun appLabel(context: Context, packageName: String): String = runCatching {
    val pm = context.packageManager
    pm.getApplicationLabel(pm.getApplicationInfo(packageName, 0)).toString()
}.getOrDefault(packageName)
