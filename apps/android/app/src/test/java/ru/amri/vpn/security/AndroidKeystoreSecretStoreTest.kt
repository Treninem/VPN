package ru.amri.vpn.security

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class AndroidKeystoreSecretStoreTest {
    @Test
    fun logicalSecretKeyIsHashedBeforePersistence() {
        val logical = "subscription:primary"
        val storage = SecureKeyCodec.storageKeyFor(logical)

        assertTrue(storage.startsWith("s1_"))
        assertEquals(67, storage.length)
        assertFalse(storage.contains("subscription"))
        assertFalse(storage.contains("primary"))
        assertEquals(storage, SecureKeyCodec.storageKeyFor(logical))
    }

    @Test
    fun invalidLogicalKeyIsRejected() {
        assertThrows(IllegalArgumentException::class.java) {
            SecureKeyCodec.storageKeyFor("../escape")
        }
    }

    @Test
    fun secretEnvelopeRoundTripsWithoutMetadataLoss() {
        val iv = byteArrayOf(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12)
        val ciphertext = ByteArray(32) { index -> (index + 20).toByte() }

        val decoded = SecretEnvelopeCodec.decode(SecretEnvelopeCodec.encode(iv, ciphertext))

        assertArrayEquals(iv, decoded.iv)
        assertArrayEquals(ciphertext, decoded.ciphertext)
    }

    @Test
    fun truncatedOrUnknownEnvelopeFailsClosed() {
        assertThrows(IllegalArgumentException::class.java) {
            SecretEnvelopeCodec.decode(byteArrayOf(1, 12, 1, 2, 3))
        }

        val iv = ByteArray(12) { 7 }
        val ciphertext = ByteArray(16) { 9 }
        val encoded = SecretEnvelopeCodec.encode(iv, ciphertext)
        encoded[0] = 2

        assertThrows(IllegalArgumentException::class.java) {
            SecretEnvelopeCodec.decode(encoded)
        }
    }
}
