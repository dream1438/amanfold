package com.amanfold.app.autofill

import android.app.PendingIntent
import android.content.Intent
import android.os.CancellationSignal
import android.service.autofill.AutofillService
import android.service.autofill.Dataset
import android.service.autofill.FillCallback
import android.service.autofill.FillRequest
import android.service.autofill.FillResponse
import android.service.autofill.SaveCallback
import android.service.autofill.SaveInfo
import android.service.autofill.SaveRequest
import android.view.autofill.AutofillId
import android.widget.RemoteViews
import com.amanfold.app.R
import com.amanfold.client.autofill.FieldRole
import com.amanfold.client.autofill.FillPolicy
import com.amanfold.client.autofill.FillTarget
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap

/**
 * Android autofill. The service never puts a secret into a suggestion: it
 * offers one "Fill with Amanfold" entry, and only after the person picks a
 * login or card in [FillActivity] (unlocking first if needed) does a value
 * leave the vault, and only into the fields [FillPolicy] allows.
 */
class AmanfoldAutofillService : AutofillService() {

    override fun onFillRequest(request: FillRequest, cancellationSignal: CancellationSignal, callback: FillCallback) {
        val structure = request.fillContexts.lastOrNull()?.structure ?: return callback.onSuccess(null)
        val screen = parseStructure(structure)
        val plan = FillPolicy.plan(screen.packageName, packageName, screen.nodes) ?: return callback.onSuccess(null)

        val fill = LinkedHashMap<Int, FieldRole>()
        fill += plan.loginFields
        fill += plan.cardFields
        plan.otpFields.forEach { fill[it] = FieldRole.OneTimeCode }
        if (fill.isEmpty()) return callback.onSuccess(null)
        val ids = ArrayList(fill.keys.map { screen.ids[it] })

        val intent = Intent(this, FillActivity::class.java).apply {
            putParcelableArrayListExtra(FillActivity.EXTRA_IDS, ids)
            putExtra(FillActivity.EXTRA_ROLES, fill.values.map { it.ordinal }.toIntArray())
            when (val t = plan.target) {
                is FillTarget.Web -> {
                    putExtra(FillActivity.EXTRA_URL, t.url)
                    putExtra(FillActivity.EXTRA_HOST, t.host)
                }
                is FillTarget.App -> putExtra(FillActivity.EXTRA_PACKAGE, t.packageName)
            }
            plan.cardFrameHost?.let { putExtra(FillActivity.EXTRA_CARD_FRAME, it) }
        }
        // Mutable: the system adds the screen's structure to the intent.
        val pi = PendingIntent.getActivity(
            this,
            UUID.randomUUID().hashCode(),
            intent,
            PendingIntent.FLAG_CANCEL_CURRENT or PendingIntent.FLAG_MUTABLE,
        )
        val presentation = RemoteViews(packageName, R.layout.autofill_item).apply {
            setTextViewText(R.id.text, getString(R.string.fill_with))
        }
        @Suppress("DEPRECATION")
        val dataset = Dataset.Builder(presentation).apply {
            ids.forEach { setValue(it, null) }
            setAuthentication(pi.intentSender)
        }.build()

        val response = FillResponse.Builder().addDataset(dataset)
        saveInfo(plan.loginFields, plan.cardFields, screen.ids)?.let(response::setSaveInfo)
        callback.onSuccess(response.build())
    }

    private fun saveInfo(login: Map<Int, FieldRole>, card: Map<Int, FieldRole>, ids: List<AutofillId>): SaveInfo? {
        val pw = login.filterValues { it == FieldRole.Password || it == FieldRole.NewPassword }.keys.map { ids[it] }
        if (pw.isNotEmpty()) {
            val user = login.filterValues { it == FieldRole.Username }.keys.map { ids[it] }
            var type = SaveInfo.SAVE_DATA_TYPE_PASSWORD
            if (user.isNotEmpty()) type = type or SaveInfo.SAVE_DATA_TYPE_USERNAME
            return SaveInfo.Builder(type, pw.toTypedArray())
                .apply { if (user.isNotEmpty()) setOptionalIds(user.toTypedArray()) }
                .setFlags(SaveInfo.FLAG_SAVE_ON_ALL_VIEWS_INVISIBLE)
                .build()
        }
        val number = card.filterValues { it == FieldRole.CardNumber }.keys.map { ids[it] }
        if (number.isNotEmpty()) {
            val rest = card.filterValues { it != FieldRole.CardNumber }.keys.map { ids[it] }
            return SaveInfo.Builder(SaveInfo.SAVE_DATA_TYPE_CREDIT_CARD, number.toTypedArray())
                .apply { if (rest.isNotEmpty()) setOptionalIds(rest.toTypedArray()) }
                .setFlags(SaveInfo.FLAG_SAVE_ON_ALL_VIEWS_INVISIBLE)
                .build()
        }
        return null
    }

    override fun onSaveRequest(request: SaveRequest, callback: SaveCallback) {
        val structure = request.fillContexts.lastOrNull()?.structure ?: return callback.onSuccess()
        val screen = parseStructure(structure)
        val plan = FillPolicy.plan(screen.packageName, packageName, screen.nodes) ?: return callback.onSuccess()
        val text = { roles: Map<Int, FieldRole>, role: FieldRole ->
            roles.filterValues { it == role }.keys.firstNotNullOfOrNull { i -> screen.nodes[i].text?.takeIf { it.isNotBlank() } }
        }
        val password = text(plan.loginFields, FieldRole.Password) ?: text(plan.loginFields, FieldRole.NewPassword)
        val save = if (password != null) {
            PendingSave.Login(plan.target, text(plan.loginFields, FieldRole.Username), password)
        } else {
            val number = text(plan.cardFields, FieldRole.CardNumber) ?: return callback.onSuccess()
            PendingSave.Card(
                number = number,
                holder = text(plan.cardFields, FieldRole.CardHolder),
                expiry = text(plan.cardFields, FieldRole.CardExpiry),
                month = text(plan.cardFields, FieldRole.CardExpMonth),
                year = text(plan.cardFields, FieldRole.CardExpYear),
                cvv = text(plan.cardFields, FieldRole.CardCvv),
            )
        }
        // The values stay in this process; the intent carries only a token.
        val token = PendingSave.put(save)
        val pi = PendingIntent.getActivity(
            this,
            token.hashCode(),
            Intent(this, SaveActivity::class.java).putExtra(SaveActivity.EXTRA_TOKEN, token),
            PendingIntent.FLAG_CANCEL_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        callback.onSuccess(pi.intentSender)
    }
}

/** A login or card the person may want to save, held in memory for [SaveActivity]. */
sealed interface PendingSave {
    data class Login(val target: FillTarget, val username: String?, val password: String) : PendingSave
    data class Card(
        val number: String,
        val holder: String?,
        val expiry: String?,
        val month: String?,
        val year: String?,
        val cvv: String?,
    ) : PendingSave

    companion object {
        private val pending = ConcurrentHashMap<String, Pair<Long, PendingSave>>()

        fun put(s: PendingSave): String {
            val now = System.currentTimeMillis()
            pending.entries.removeIf { now - it.value.first > 10 * 60_000 }
            val token = UUID.randomUUID().toString()
            pending[token] = now to s
            return token
        }

        fun take(token: String?): PendingSave? = token?.let { pending.remove(it)?.second }
    }
}
