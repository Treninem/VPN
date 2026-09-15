package ru.amri.vpn.security

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.nio.ByteBuffer
import java.security.KeyStore
import java.security.MessageDigest
import java.util.Base64
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Android secure persistence boundary for AMRI credential-bearing values.
 *
 * Plaintext values are encrypted with an AES-256 key generated inside Android Keystore. Only the
 * versioned AES-GCM ciphertext envelope is written to SharedPreferences. Logical secret names are
 * SHA-256 hashed before they become preference keys, so labels/subscription IDs are not disclosed
 * by the preferences file.
 */
class AndroidKeystoreSecretStore(context: Context) {
    private val preferences = context.applicationContext.getSharedPreferences(
        PREFERENCES_NAME,
        Context.MODE_PRIVATE,
    )

    fun put(logicalKey: String, plaintext: ByteArray) {
        require(plaintext.isNotEmpty()) { "secret value is empty" }
        val storageKey = SecureKeyCodec.storageKeyFor(logicalKey)
        val key = loadOrCreateMasterKey()
        val cipher = Cipher.getInstance(CIPHER_TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, key)
        val ciphertext = cipher.doFinal(plaintext)
        val encodedEnvelope = Base64.getEncoder().withoutPadding().encodeToString(
            SecretEnvelopeCodec.encode(cipher.iv, ciphertext),
        )

        if (!preferences.edit().putString(storageKey, encodedEnvelope).commit()) {
            throw SecureStoreException("secure storage write failed")
        }
    }

    fun get(logicalKey: String): ByteArray? {
        val storageKey = SecureKeyCodec.storageKeyFor(logicalKey)
        val encodedEnvelope = preferences.getString(storageKey, null) ?: return null

        return try {
            val envelope = Base64.getDecoder().decode(encodedEnvelope)
            val decoded = SecretEnvelopeCodec.decode(envelope)
            val cipher = Cipher.getInstance(CIPHER_TRANSFORMATION)
            cipher.init(
                Cipher.DECRYPT_MODE,
                loadOrCreateMasterKey(),
                GCMParameterSpec(GCM_TAG_BITS, decoded.iv),
            )
            cipher.doFinal(decoded.ciphertext)
        } catch (_: Exception) {
            throw SecureStoreException("secure storage read failed")
        }
    }

    fun delete(logicalKey: String): Boolean {
        val storageKey = SecureKeyCodec.storageKeyFor(logicalKey)
        if (!preferences.contains(storageKey)) {
            return false
        }
        if (!preferences.edit().remove(storageKey).commit()) {
            throw SecureStoreException("secure storage delete failed")
        }
        return true
    }

    private fun loadOrCreateMasterKey(): SecretKey = synchronized(KEY_LOCK) {
        try {
            val keyStore = KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }
            val existing = keyStore.getKey(MASTER_KEY_ALIAS, null) as? SecretKey
            if (existing != null) {
                return@synchronized existing
            }

            val keyGenerator = KeyGenerator.getInstance(
                KeyProperties.KEY_ALGORITHM_AES,
                ANDROID_KEYSTORE,
            )
            keyGenerator.init(
                KeyGenParameterSpec.Builder(
                    MASTER_KEY_ALIAS,
                    KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
                )
                    .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                    .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                    .setKeySize(256)
                    .setRandomizedEncryptionRequired(true)
                    .setUserAuthenticationRequired(false)
                    .build(),
            )
            keyGenerator.generateKey()
        } catch (_: Exception) {
            throw SecureStoreException("Android Keystore is unavailable")
        }
    }

    companion object {
        private const val PREFERENCES_NAME = "amri_secure_store_v1"
        private const val ANDROID_KEYSTORE = "AndroidKeyStore"
        private const val MASTER_KEY_ALIAS = "ru.amri.vpn.secret.master.v1"
        private const val CIPHER_TRANSFORMATION = "AES/GCM/NoPadding"
        private const val GCM_TAG_BITS = 128
        private val KEY_LOCK = Any()
    }
}

class SecureStoreException(message: String) : IllegalStateException(message)

internal object SecureKeyCodec {
    private const val PREFIX = "s1_"

    fun storageKeyFor(logicalKey: String): String {
        validateLogicalKey(logicalKey)
        val digest = MessageDigest.getInstance("SHA-256").digest(logicalKey.toByteArray(Charsets.UTF_8))
        return PREFIX + digest.joinToString(separator = "") { byte ->
            "%02x".format(byte.toInt() and 0xff)
        }
    }

    private fun validateLogicalKey(logicalKey: String) {
        require(logicalKey.isNotEmpty() && logicalKey.length <= 256) { "secret key is invalid" }
        require(
            logicalKey.all { character ->
                character in 'a'..'z' ||
                    character in 'A'..'Z' ||
                    character in '0'..'9' ||
                    character in charArrayOf('-', '_', '.', ':')
            },
        ) { "secret key is invalid" }
    }
}

internal data class SecretEnvelope(
    val iv: ByteArray,
    val ciphertext: ByteArray,
)

internal object SecretEnvelopeCodec {
    private const val VERSION: Byte = 1
    private const val HEADER_BYTES = 2
    private const val MIN_GCM_CIPHERTEXT_BYTES = 16
    private const val MAX_IV_BYTES = 32

    fun encode(iv: ByteArray, ciphertext: ByteArray): ByteArray {
        require(iv.isNotEmpty() && iv.size <= MAX_IV_BYTES) { "invalid secret envelope" }
        require(ciphertext.size >= MIN_GCM_CIPHERTEXT_BYTES) { "invalid secret envelope" }
        return ByteBuffer.allocate(HEADER_BYTES + iv.size + ciphertext.size)
            .put(VERSION)
            .put(iv.size.toByte())
            .put(iv)
            .put(ciphertext)
            .array()
    }

    fun decode(envelope: ByteArray): SecretEnvelope {
        require(envelope.size >= HEADER_BYTES + 1 + MIN_GCM_CIPHERTEXT_BYTES) {
            "invalid secret envelope"
        }
        val buffer = ByteBuffer.wrap(envelope)
        require(buffer.get() == VERSION) { "unsupported secret envelope" }
        val ivLength = buffer.get().toInt() and 0xff
        require(ivLength in 1..MAX_IV_BYTES) { "invalid secret envelope" }
        require(buffer.remaining() >= ivLength + MIN_GCM_CIPHERTEXT_BYTES) {
            "invalid secret envelope"
        }

        val iv = ByteArray(ivLength)
        buffer.get(iv)
        val ciphertext = ByteArray(buffer.remaining())
        buffer.get(ciphertext)
        return SecretEnvelope(iv, ciphertext)
    }
}
