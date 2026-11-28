package com.amanfold.app.security

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import com.amanfold.client.BlobCipher
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Encrypts the stored account at rest with an AES-256-GCM key that lives in
 * the Android Keystore (in secure hardware where the phone has it) and never
 * leaves it. No user authentication: the vault inside is already encrypted
 * under the master password; this keeps the Secret Key and tokens off disk
 * in the clear.
 */
class KeystoreCipher(private val alias: String) : BlobCipher {
    private fun key(): SecretKey {
        val ks = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (ks.getKey(alias, null) as? SecretKey)?.let { return it }
        val gen = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        gen.init(
            KeyGenParameterSpec.Builder(alias, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        return gen.generateKey()
    }

    override fun seal(plain: ByteArray): ByteArray {
        val c = Cipher.getInstance("AES/GCM/NoPadding")
        c.init(Cipher.ENCRYPT_MODE, key())
        val iv = c.iv
        val ct = c.doFinal(plain)
        return byteArrayOf(iv.size.toByte()) + iv + ct
    }

    override fun open(sealed: ByteArray): ByteArray {
        val n = sealed[0].toInt()
        val iv = sealed.copyOfRange(1, 1 + n)
        val c = Cipher.getInstance("AES/GCM/NoPadding")
        c.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, iv))
        return c.doFinal(sealed, 1 + n, sealed.size - 1 - n)
    }
}
