package com.amanfold.client.autofill

import com.amanfold.core.pageSite

// Autofill decisions that do not need Android: which fields on a screen are
// which, and which saved items may be offered there. The Android service
// turns its AssistStructure into [FieldNode]s and acts on the [FillPlan].

/** What a field is for. */
enum class FieldRole {
    Username,
    Password,
    NewPassword,
    OneTimeCode,
    CardNumber,
    CardExpiry,
    CardExpMonth,
    CardExpYear,
    CardCvv,
    CardHolder,
}

val CARD_ROLES = setOf(
    FieldRole.CardNumber, FieldRole.CardExpiry, FieldRole.CardExpMonth,
    FieldRole.CardExpYear, FieldRole.CardCvv, FieldRole.CardHolder,
)
val LOGIN_ROLES = setOf(FieldRole.Username, FieldRole.Password, FieldRole.NewPassword)

/** One editable view, as the platform describes it. */
data class FieldNode(
    /** Index into the platform's own list of nodes. */
    val index: Int,
    val autofillHints: List<String> = emptyList(),
    /** Android `InputType` bits. */
    val inputType: Int = 0,
    /** For web pages: the `<input>` attributes (type, name, id, autocomplete, placeholder, aria-label). */
    val htmlAttributes: Map<String, String> = emptyMap(),
    val idEntry: String? = null,
    val hint: String? = null,
    val label: String? = null,
    /** Domain of the document the field is in (web pages only). */
    val webDomain: String? = null,
    val webScheme: String? = null,
    val focused: Boolean = false,
    val visible: Boolean = true,
    /** Current text, used only by the save flow. */
    val text: String? = null,
)

private object InputType {
    const val CLASS_MASK = 0x0000000f
    const val VARIATION_MASK = 0x00000ff0
    const val CLASS_TEXT = 1
    const val CLASS_NUMBER = 2
    const val TEXT_PASSWORD = 0x80
    const val TEXT_VISIBLE_PASSWORD = 0x90
    const val TEXT_WEB_PASSWORD = 0xe0
    const val TEXT_EMAIL = 0x20
    const val TEXT_WEB_EMAIL = 0xd0
    const val NUMBER_PASSWORD = 0x10
}

private val hintRoles: Map<String, FieldRole> = mapOf(
    // android.view.View AUTOFILL_HINT_* and androidx.autofill HintConstants
    "username" to FieldRole.Username,
    "emailaddress" to FieldRole.Username,
    "email" to FieldRole.Username,
    "password" to FieldRole.Password,
    "newpassword" to FieldRole.NewPassword,
    "newusername" to FieldRole.Username,
    "smsotpcode" to FieldRole.OneTimeCode,
    "2faappotpcode" to FieldRole.OneTimeCode,
    "otpcode" to FieldRole.OneTimeCode,
    "creditcardnumber" to FieldRole.CardNumber,
    "creditcardexpirationdate" to FieldRole.CardExpiry,
    "creditcardexpirationmonth" to FieldRole.CardExpMonth,
    "creditcardexpirationyear" to FieldRole.CardExpYear,
    "creditcardsecuritycode" to FieldRole.CardCvv,
    // HTML autocomplete tokens
    "current-password" to FieldRole.Password,
    "new-password" to FieldRole.NewPassword,
    "one-time-code" to FieldRole.OneTimeCode,
    "cc-number" to FieldRole.CardNumber,
    "cc-exp" to FieldRole.CardExpiry,
    "cc-exp-month" to FieldRole.CardExpMonth,
    "cc-exp-year" to FieldRole.CardExpYear,
    "cc-csc" to FieldRole.CardCvv,
    "cc-name" to FieldRole.CardHolder,
)

// Words in ids, names, hints and labels, in English and Arabic. Order
// matters: the first match wins, so specific roles come first.
private val wordRoles: List<Pair<Regex, FieldRole>> = listOf(
    Regex("cvv|cvc|csc|security.?code|card.?code|رمز.?الأمان|رمز.?التحقق.?من.?البطاقة") to FieldRole.CardCvv,
    Regex("card.?holder|name.?on.?card|cc.?name|الاسم.?على.?البطاقة|حامل.?البطاقة") to FieldRole.CardHolder,
    Regex("card.?num|cc.?num|cardnumber|ccnumber|credit.?card|رقم.?البطاقة") to FieldRole.CardNumber,
    Regex("exp.*month|cc.?month|exp.?mm") to FieldRole.CardExpMonth,
    Regex("exp.*year|cc.?year|exp.?yy") to FieldRole.CardExpYear,
    Regex("expir|exp.?date|mm.?/.?yy|تاريخ.?الانتهاء|صلاحية") to FieldRole.CardExpiry,
    Regex("otp|one.?time|2fa|totp|verification.?code|auth.?code|رمز.?التحقق") to FieldRole.OneTimeCode,
    Regex("new.?pass|confirm.?pass|repeat.?pass|retype|كلمة.?المرور.?الجديدة|تأكيد.?كلمة") to FieldRole.NewPassword,
    Regex("pass|pwd|كلمة.?المرور|كلمة.?السر|الرقم.?السري") to FieldRole.Password,
    Regex("user|login|e.?mail|account|identifier|اسم.?المستخدم|البريد|الحساب") to FieldRole.Username,
)

/** Assigns roles to the fields of one screen. */
object FieldClassifier {
    fun classify(nodes: List<FieldNode>): Map<Int, FieldRole> {
        val out = LinkedHashMap<Int, FieldRole>()
        for (n in nodes) {
            if (!n.visible) continue
            role(n)?.let { out[n.index] = it }
        }
        // A login form without a recognised username: the text field just
        // before the first password field.
        val firstPw = nodes.indexOfFirst { out[it.index] == FieldRole.Password || out[it.index] == FieldRole.NewPassword }
        if (firstPw > 0 && out.values.none { it == FieldRole.Username }) {
            nodes.subList(0, firstPw).lastOrNull { it.visible && it.index !in out && isPlainText(it) }
                ?.let { out[it.index] = FieldRole.Username }
        }
        return out
    }

    private fun isPlainText(n: FieldNode): Boolean {
        val type = n.htmlAttributes["type"]?.lowercase()
        if (type != null) return type in setOf("text", "email", "tel", "")
        return n.inputType and InputType.CLASS_MASK == InputType.CLASS_TEXT
    }

    private fun role(n: FieldNode): FieldRole? {
        val tokens = n.autofillHints.map { it.lowercase() } +
            (n.htmlAttributes["autocomplete"]?.lowercase()?.split(' ') ?: emptyList())
        // Explicit "off" alone says nothing about the field; keep looking.
        tokens.firstNotNullOfOrNull { hintRoles[it] }?.let { return it }

        val htmlType = n.htmlAttributes["type"]?.lowercase()
        if (htmlType in setOf("hidden", "submit", "button", "checkbox", "radio", "file", "image", "reset", "search")) return null
        val words = listOfNotNull(
            n.idEntry, n.hint, n.label,
            n.htmlAttributes["name"], n.htmlAttributes["id"], n.htmlAttributes["placeholder"], n.htmlAttributes["aria-label"],
        ).joinToString(" ").lowercase()
        val byWords = wordRoles.firstOrNull { (re, _) -> re.containsMatchIn(words) }?.second

        val cls = n.inputType and InputType.CLASS_MASK
        val variation = n.inputType and InputType.VARIATION_MASK
        val passwordType = htmlType == "password" ||
            (cls == InputType.CLASS_TEXT && variation in setOf(InputType.TEXT_PASSWORD, InputType.TEXT_VISIBLE_PASSWORD, InputType.TEXT_WEB_PASSWORD)) ||
            (cls == InputType.CLASS_NUMBER && variation == InputType.NUMBER_PASSWORD)
        if (passwordType) {
            return when (byWords) {
                FieldRole.NewPassword, FieldRole.CardCvv, FieldRole.OneTimeCode -> byWords
                else -> FieldRole.Password
            }
        }
        val emailType = htmlType == "email" ||
            (cls == InputType.CLASS_TEXT && variation in setOf(InputType.TEXT_EMAIL, InputType.TEXT_WEB_EMAIL))
        if (emailType) return FieldRole.Username
        // A plain text field named "password" is usually a label mix-up; only
        // trust the words for roles that are not secrets.
        return when (byWords) {
            FieldRole.Password, FieldRole.NewPassword -> null
            else -> byWords
        }
    }
}

/** Where the fields are: a web page in a trusted browser, or an app. */
sealed interface FillTarget {
    /** A page. [url] is matched with the core's site rules. */
    data class Web(val url: String, val host: String) : FillTarget
    /** A native app, identified later by package name and signing certificate. */
    data class App(val packageName: String) : FillTarget
}

/** What the service may offer on one screen. */
data class FillPlan(
    val target: FillTarget,
    /** Login fields the target's logins may go into. */
    val loginFields: Map<Int, FieldRole>,
    /** Card fields, by the host of the document they are in. */
    val cardFields: Map<Int, FieldRole>,
    /** Host of card fields that sit in a frame from another site (a payment provider): fill only after the person agrees. */
    val cardFrameHost: String?,
    val otpFields: List<Int>,
)

object FillPolicy {
    /**
     * Browsers whose reported page address is trusted. Any other app could
     * claim to show bank.com, so for them only the app's own identity counts.
     * Open item: pin their signing certificates too (docs/SECURITY.md, A2).
     */
    val TRUSTED_BROWSERS = setOf(
        "com.android.chrome", "com.chrome.beta", "com.chrome.dev", "com.chrome.canary",
        "org.mozilla.firefox", "org.mozilla.firefox_beta", "org.mozilla.fenix", "org.mozilla.focus",
        "com.microsoft.emmx", "com.brave.browser", "com.duckduckgo.mobile.android",
        "com.sec.android.app.sbrowser", "com.opera.browser", "com.vivaldi.browser",
        "com.kiwibrowser.browser", "org.chromium.chrome",
    )

    /**
     * Decide what may be filled. `null` when nothing should be offered: no
     * fields, our own app, or a browser that did not say which page it shows.
     */
    fun plan(packageName: String, ownPackage: String, nodes: List<FieldNode>): FillPlan? {
        if (packageName == ownPackage || packageName.startsWith("$ownPackage.")) return null
        val roles = FieldClassifier.classify(nodes)
        if (roles.isEmpty()) return null
        val byIndex = nodes.associateBy { it.index }
        val browser = packageName in TRUSTED_BROWSERS

        if (!browser) {
            return FillPlan(
                target = FillTarget.App(packageName),
                loginFields = roles.filterValues { it in LOGIN_ROLES },
                cardFields = roles.filterValues { it in CARD_ROLES },
                cardFrameHost = null,
                otpFields = roles.filterValues { it == FieldRole.OneTimeCode }.keys.toList(),
            )
        }

        // The page itself: the first node that names a domain is the top document.
        val top = nodes.firstOrNull { it.webDomain != null } ?: return null
        val topSite = siteOf(top) ?: return null
        val focusedDomainNode = nodes.firstOrNull { it.focused && it.webDomain != null && it.index in roles }
            ?: nodes.firstOrNull { it.webDomain != null && it.index in roles }
            ?: return null
        val loginHost = focusedDomainNode.webDomain!!.lowercase()
        val loginUrl = urlOf(focusedDomainNode)
        val loginSite = pageSite(loginUrl)?.site

        // Logins go only into fields of the focused field's document, and
        // only when that document is on the top page's site (no logins into
        // a frame from another site, nor into a frame embedded by one).
        val loginFields = if (loginSite == topSite) {
            roles.filter { (i, r) -> r in LOGIN_ROLES && byIndex[i]?.webDomain?.lowercase() == loginHost }
        } else emptyMap()
        val otp = roles.filter { (i, r) -> r == FieldRole.OneTimeCode && byIndex[i]?.webDomain?.lowercase() == loginHost }.keys

        // Cards have no site. All card fields must be in one document; one
        // from another site needs the person's yes.
        val cards = roles.filterValues { it in CARD_ROLES }
        val cardHosts = cards.keys.mapNotNull { byIndex[it]?.webDomain?.lowercase() }.toSet()
        val (cardFields, cardFrameHost) = when {
            cards.isEmpty() -> emptyMap<Int, FieldRole>() to null
            cardHosts.size != 1 -> {
                // Split across documents: keep the focused one's.
                val host = (nodes.firstOrNull { it.focused && it.index in cards }?.webDomain ?: cardHosts.first()).lowercase()
                cards.filterKeys { byIndex[it]?.webDomain?.lowercase() == host } to crossSiteHost(host, topSite)
            }
            else -> cards to crossSiteHost(cardHosts.first(), topSite)
        }
        if (loginFields.isEmpty() && cardFields.isEmpty() && otp.isEmpty()) return null
        return FillPlan(
            target = FillTarget.Web(loginUrl, loginHost),
            loginFields = loginFields,
            cardFields = cardFields,
            cardFrameHost = cardFrameHost,
            otpFields = otp.toList(),
        )
    }

    private fun crossSiteHost(host: String, topSite: String): String? =
        if (pageSite("https://$host")?.site == topSite) null else host

    private fun urlOf(n: FieldNode): String {
        // Browsers that do not report the scheme are assumed to show https.
        // Open item A3 in docs/SECURITY.md.
        val scheme = n.webScheme?.lowercase()?.takeIf { it == "http" || it == "https" } ?: "https"
        return "$scheme://${n.webDomain!!.lowercase()}"
    }

    private fun siteOf(n: FieldNode): String? = pageSite(urlOf(n))?.site
}
