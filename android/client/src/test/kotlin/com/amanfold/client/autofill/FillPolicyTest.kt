package com.amanfold.client.autofill

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class FillPolicyTest {
    private val own = "com.amanfold.app"
    private val textPassword = 0x81
    private val text = 0x1
    private val email = 0x21

    @Test
    fun nativeLoginFormByInputType() {
        val nodes = listOf(
            FieldNode(0, inputType = text, idEntry = "search_box", hint = "Search"),
            FieldNode(1, inputType = text, idEntry = "et_phone"),
            FieldNode(2, inputType = textPassword, idEntry = "et_secret"),
        )
        val roles = FieldClassifier.classify(nodes)
        assertEquals(mapOf(1 to FieldRole.Username, 2 to FieldRole.Password), roles)
        val plan = FillPolicy.plan("com.bank.mobile", own, nodes)!!
        assertEquals(FillTarget.App("com.bank.mobile"), plan.target)
        assertEquals(setOf(1, 2), plan.loginFields.keys)
    }

    @Test
    fun hintsAndArabicLabels() {
        val nodes = listOf(
            FieldNode(0, autofillHints = listOf("emailAddress")),
            FieldNode(1, inputType = textPassword, hint = "كلمة المرور الجديدة"),
            FieldNode(2, hint = "رقم البطاقة"),
            FieldNode(3, hint = "رمز الأمان", inputType = 0x12),
            FieldNode(4, htmlAttributes = mapOf("autocomplete" to "cc-exp")),
            FieldNode(5, htmlAttributes = mapOf("type" to "text", "name" to "otp")),
            FieldNode(6, inputType = text, hint = "Name on card"),
            FieldNode(7, inputType = text, hint = "Password"), // plain text: not a secret field
            FieldNode(8, inputType = email, visible = false),
        )
        assertEquals(
            mapOf(
                0 to FieldRole.Username,
                1 to FieldRole.NewPassword,
                2 to FieldRole.CardNumber,
                3 to FieldRole.CardCvv,
                4 to FieldRole.CardExpiry,
                5 to FieldRole.OneTimeCode,
                6 to FieldRole.CardHolder,
            ),
            FieldClassifier.classify(nodes),
        )
    }

    @Test
    fun ownAppAndEmptyScreensGetNothing() {
        val pw = listOf(FieldNode(0, inputType = textPassword))
        assertNull(FillPolicy.plan(own, own, pw))
        assertNull(FillPolicy.plan("$own.debug", own, pw))
        assertNull(FillPolicy.plan("com.other", own, listOf(FieldNode(0, inputType = text, hint = "Search"))))
    }

    @Test
    fun untrustedAppClaimingAWebDomainIsTreatedAsItself() {
        val nodes = listOf(
            FieldNode(0, inputType = email, webDomain = "bank.com", webScheme = "https"),
            FieldNode(1, inputType = textPassword, webDomain = "bank.com", webScheme = "https"),
        )
        assertEquals(FillTarget.App("com.evil.webview"), FillPolicy.plan("com.evil.webview", own, nodes)!!.target)
    }

    @Test
    fun browserPageWithSameSiteAndCrossSiteFrames() {
        val nodes = listOf(
            FieldNode(0, webDomain = "www.bank.com", webScheme = "https", htmlAttributes = mapOf("type" to "search", "name" to "q")),
            FieldNode(1, webDomain = "login.bank.com", webScheme = "https", focused = true, htmlAttributes = mapOf("type" to "email", "name" to "user")),
            FieldNode(2, webDomain = "login.bank.com", webScheme = "https", htmlAttributes = mapOf("type" to "password")),
            // A frame from another site with a password field never gets the login.
            FieldNode(3, webDomain = "evil.example", webScheme = "https", htmlAttributes = mapOf("type" to "password")),
        )
        val plan = FillPolicy.plan("com.android.chrome", own, nodes)!!
        assertEquals(FillTarget.Web("https://login.bank.com", "login.bank.com"), plan.target)
        assertEquals(setOf(1, 2), plan.loginFields.keys)
    }

    @Test
    fun loginFrameFromAnotherSiteGetsNoLogin() {
        val nodes = listOf(
            FieldNode(0, webDomain = "news.example", webScheme = "https", htmlAttributes = mapOf("type" to "text", "name" to "q", "placeholder" to "search")),
            FieldNode(1, webDomain = "bank.com", webScheme = "https", focused = true, htmlAttributes = mapOf("type" to "email")),
            FieldNode(2, webDomain = "bank.com", webScheme = "https", htmlAttributes = mapOf("type" to "password")),
        )
        assertNull(FillPolicy.plan("com.android.chrome", own, nodes))
    }

    @Test
    fun cardsInPaymentFrameNeedConfirmation() {
        val nodes = listOf(
            FieldNode(0, webDomain = "shop.example", webScheme = "https", htmlAttributes = mapOf("autocomplete" to "cc-name")),
            FieldNode(1, webDomain = "js.stripe.com", webScheme = "https", focused = true, htmlAttributes = mapOf("autocomplete" to "cc-number")),
            FieldNode(2, webDomain = "js.stripe.com", webScheme = "https", htmlAttributes = mapOf("autocomplete" to "cc-csc")),
        )
        val plan = FillPolicy.plan("com.android.chrome", own, nodes)!!
        assertEquals(setOf(1, 2), plan.cardFields.keys)
        assertEquals("js.stripe.com", plan.cardFrameHost)
        assertEquals(emptyMap(), plan.loginFields)

        val sameSite = FillPolicy.plan(
            "com.android.chrome", own,
            listOf(
                FieldNode(0, webDomain = "shop.example", webScheme = "https", focused = true, htmlAttributes = mapOf("autocomplete" to "cc-number")),
                FieldNode(1, webDomain = "pay.shop.example", webScheme = "https", htmlAttributes = mapOf("autocomplete" to "cc-csc")),
            ),
        )!!
        assertNull(sameSite.cardFrameHost)
        assertEquals(setOf(0), sameSite.cardFields.keys)
    }

    @Test
    fun browserWithoutPageAddressGetsNothing() {
        assertNull(FillPolicy.plan("com.android.chrome", own, listOf(FieldNode(0, inputType = textPassword))))
    }

    @Test
    fun httpPageKeepsItsScheme() {
        val nodes = listOf(FieldNode(0, webDomain = "bank.com", webScheme = "http", htmlAttributes = mapOf("type" to "password")))
        assertEquals("http://bank.com", (FillPolicy.plan("org.mozilla.firefox", own, nodes)!!.target as FillTarget.Web).url)
    }
}
