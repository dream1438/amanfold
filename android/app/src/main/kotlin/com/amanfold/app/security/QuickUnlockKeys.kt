package com.amanfold.app.security

import android.content.Context
import android.os.Build
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyPermanentlyInvalidatedException
import android.security.keystore.KeyProperties
import android.util.Base64
import androidx.biometric.BiometricManager
import androidx.biometric.BiometricManager.Authenticators.BIOMETRIC_STRONG
import androidx.biometric.BiometricPrompt
import androidx.core.content.ContextCompat
import androidx.fragment.app.FragmentActivity
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import kotlin.coroutines.resume
import kotlinx.coroutines.suspendCancellableCoroutine

/**
 * Quick unlock: the core's device key, encrypted with a Keystore key that
 * only a strong biometric check (fingerprint or face, class 3) can use, once
 * per unlock. Enrolling a new fingerprint or face destroys the key, so quick
 * unlock turns off and the master password is needed again.
 */
class QuickUnlockKeys(context: Context) {
    private val alias = "amanfold.quickunlock"
    private val prefs = context.getSharedPreferences("quick", Context.MODE_PRIVATE)
    private val appContext = context.applicationContext

    fun available(): Boolean =
        BiometricManager.from(appContext).canAuthenticate(BIOMETRIC_STRONG) == BiometricManager.BIOMETRIC_SUCCESS

    fun hasKey(): Boolean = prefs.contains("ct")

    private fun keystore() = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }

    private fun newKey(): SecretKey {
        keystore().deleteEntry(alias)
        val spec = KeyGenParameterSpec.Builder(alias, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setKeySize(256)
            .setUserAuthenticationRequired(true)
            .setInvalidatedByBiometricEnrollment(true)
            .apply {
                if (Build.VERSION.SDK_INT >= 30) {
                    setUserAuthenticationParameters(0, KeyProperties.AUTH_BIOMETRIC_STRONG)
                }
            }
            .build()
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").run {
            init(spec)
            generateKey()
        }
    }

    /** A cipher to encrypt a new device key; the biometric check unlocks it. */
    fun encryptCipher(): Cipher = Cipher.getInstance("AES/GCM/NoPadding").apply {
        init(Cipher.ENCRYPT_MODE, newKey())
    }

    /**
     * A cipher to decrypt the stored device key, or `null` when there is
     * none or the biometrics changed (then everything is cleared).
     */
    fun decryptCipher(): Cipher? {
        val iv = prefs.getString("iv", null)?.let { Base64.decode(it, Base64.NO_WRAP) } ?: return null
        val key = keystore().getKey(alias, null) as? SecretKey ?: run { clear(); return null }
        return try {
            Cipher.getInstance("AES/GCM/NoPadding").apply { init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(128, iv)) }
        } catch (e: KeyPermanentlyInvalidatedException) {
            clear()
            null
        }
    }

    fun save(authenticated: Cipher, deviceKey: ByteArray) {
        val ct = authenticated.doFinal(deviceKey)
        prefs.edit()
            .putString("iv", Base64.encodeToString(authenticated.iv, Base64.NO_WRAP))
            .putString("ct", Base64.encodeToString(ct, Base64.NO_WRAP))
            .apply()
        deviceKey.fill(0)
    }

    fun open(authenticated: Cipher): ByteArray {
        val ct = Base64.decode(prefs.getString("ct", null) ?: error("no quick unlock"), Base64.NO_WRAP)
        return authenticated.doFinal(ct)
    }

    fun clear() {
        prefs.edit().clear().apply()
        runCatching { keystore().deleteEntry(alias) }
    }
}

/**
 * Shows the system biometric prompt for [cipher]. Returns the unlocked cipher,
 * or `null` if the person cancelled or chose the master password instead.
 */
suspend fun FragmentActivity.authenticate(title: String, negative: String, cipher: Cipher): Cipher? =
    suspendCancellableCoroutine { cont ->
        val prompt = BiometricPrompt(
            this,
            ContextCompat.getMainExecutor(this),
            object : BiometricPrompt.AuthenticationCallback() {
                override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) {
                    if (cont.isActive) cont.resume(result.cryptoObject?.cipher)
                }

                override fun onAuthenticationError(errorCode: Int, errString: CharSequence) {
                    if (cont.isActive) cont.resume(null)
                }
            },
        )
        val info = BiometricPrompt.PromptInfo.Builder()
            .setTitle(title)
            .setNegativeButtonText(negative)
            .setAllowedAuthenticators(BIOMETRIC_STRONG)
            .setConfirmationRequired(false)
            .build()
        prompt.authenticate(info, BiometricPrompt.CryptoObject(cipher))
        cont.invokeOnCancellation { prompt.cancelAuthentication() }
    }
