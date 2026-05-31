package expo.modules.syncminddeviceidentity

import android.content.Context
import android.os.Build
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import expo.modules.kotlin.exception.CodedException
import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition
import java.security.KeyStore
import java.security.MessageDigest
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import org.bouncycastle.crypto.params.Ed25519PrivateKeyParameters
import org.bouncycastle.crypto.params.X25519PrivateKeyParameters
import org.bouncycastle.crypto.params.X25519PublicKeyParameters
import org.bouncycastle.crypto.signers.Ed25519Signer

class SyncMindDeviceIdentityModule : Module() {
  private val store by lazy {
    val context = appContext.reactContext?.applicationContext
      ?: throw CodedException("React context is not available")
    DeviceIdentityStore(context)
  }

  override fun definition() = ModuleDefinition {
    Name("SyncMindDeviceIdentity")

    AsyncFunction("ensureIdentity") {
      store.ensureIdentity()
    }

    AsyncFunction("getIdentityMeta") {
      store.getIdentityMeta()
    }

    AsyncFunction("sign") { messageBase64: String ->
      store.sign(messageBase64)
    }

    AsyncFunction("deriveX25519") { peerPubKeyHex: String ->
      store.deriveX25519(peerPubKeyHex)
    }

    AsyncFunction("setBiometricProtection") { enabled: Boolean ->
      store.setBiometricProtection(enabled)
    }

    AsyncFunction("resetIdentity") {
      store.resetIdentity()
    }

    AsyncFunction("importLegacyIdentity") { privateKeyHex: String ->
      store.importLegacyIdentity(privateKeyHex)
    }
  }
}

private class DeviceIdentityStore(private val context: Context) {
  private val prefs = context.getSharedPreferences("syncmind_device_identity", Context.MODE_PRIVATE)
  private val secureRandom = SecureRandom()

  fun ensureIdentity(): Map<String, Any> {
    getIdentityMeta()?.let { return it }

    val seed = if (prefs.contains(KEY_ENCRYPTED_SEED)) {
      decryptSeed()
    } else {
      ByteArray(32).also { secureRandom.nextBytes(it) }
    }

    saveIdentity(seed, biometricEnabled = false)
    return metadata(seed, biometricEnabled = false)
  }

  fun getIdentityMeta(): Map<String, Any>? {
    if (!prefs.contains(KEY_ENCRYPTED_SEED)) {
      return null
    }

    val fingerprint = prefs.getString(KEY_FINGERPRINT, null) ?: return null
    val publicKeyHex = prefs.getString(KEY_PUBLIC_KEY, null) ?: return null

    return mapOf(
      "fingerprint" to fingerprint,
      "publicKeyHex" to publicKeyHex,
      "biometricEnabled" to prefs.getBoolean(KEY_BIOMETRIC_ENABLED, false),
    )
  }

  fun sign(messageBase64: String): String {
    val message = Base64.decode(messageBase64, Base64.NO_WRAP)
    val signer = Ed25519Signer()
    signer.init(true, Ed25519PrivateKeyParameters(decryptSeed(), 0))
    signer.update(message, 0, message.size)
    return Base64.encodeToString(signer.generateSignature(), Base64.NO_WRAP)
  }

  fun deriveX25519(peerPubKeyHex: String): String {
    val peer = hexToBytes(peerPubKeyHex)
    if (peer.size != 32) {
      throw CodedException("Invalid X25519 peer public key")
    }

    val privateKey = X25519PrivateKeyParameters(seedToX25519Scalar(decryptSeed()), 0)
    val publicKey = X25519PublicKeyParameters(peer, 0)
    val secret = ByteArray(32)
    privateKey.generateSecret(publicKey, secret, 0)
    return Base64.encodeToString(secret, Base64.NO_WRAP)
  }

  fun setBiometricProtection(enabled: Boolean) {
    val seed = decryptSeed()
    saveIdentity(seed, biometricEnabled = enabled)
  }

  fun resetIdentity() {
    keyStore().deleteEntry(KEY_ALIAS)
    prefs.edit().clear().apply()
  }

  fun importLegacyIdentity(privateKeyHex: String): Map<String, Any> {
    val seed = hexToBytes(privateKeyHex)
    if (seed.size != 32) {
      throw CodedException("Invalid device identity seed")
    }

    saveIdentity(seed, biometricEnabled = false)
    return metadata(seed, biometricEnabled = false)
  }

  private fun saveIdentity(seed: ByteArray, biometricEnabled: Boolean) {
    keyStore().deleteEntry(KEY_ALIAS)
    generateWrapKey(biometricEnabled)

    val cipher = Cipher.getInstance(AES_TRANSFORMATION)
    cipher.init(Cipher.ENCRYPT_MODE, wrapKey())
    val encrypted = cipher.doFinal(seed)
    val meta = metadata(seed, biometricEnabled)

    prefs.edit()
      .putString(KEY_ENCRYPTED_SEED, Base64.encodeToString(encrypted, Base64.NO_WRAP))
      .putString(KEY_IV, Base64.encodeToString(cipher.iv, Base64.NO_WRAP))
      .putString(KEY_FINGERPRINT, meta["fingerprint"] as String)
      .putString(KEY_PUBLIC_KEY, meta["publicKeyHex"] as String)
      .putBoolean(KEY_BIOMETRIC_ENABLED, biometricEnabled)
      .apply()
  }

  private fun decryptSeed(): ByteArray {
    val encrypted = prefs.getString(KEY_ENCRYPTED_SEED, null)
      ?: throw CodedException("Device identity not initialized")
    val iv = prefs.getString(KEY_IV, null)
      ?: throw CodedException("Device identity IV missing")

    val cipher = Cipher.getInstance(AES_TRANSFORMATION)
    cipher.init(
      Cipher.DECRYPT_MODE,
      wrapKey(),
      GCMParameterSpec(128, Base64.decode(iv, Base64.NO_WRAP)),
    )
    return cipher.doFinal(Base64.decode(encrypted, Base64.NO_WRAP))
  }

  private fun metadata(seed: ByteArray, biometricEnabled: Boolean): Map<String, Any> {
    val publicKey = Ed25519PrivateKeyParameters(seed, 0).generatePublicKey().encoded
    val publicKeyHex = bytesToHex(publicKey)
    val fingerprint = "sha256:${bytesToHex(MessageDigest.getInstance("SHA-256").digest(publicKey))}"

    return mapOf(
      "fingerprint" to fingerprint,
      "publicKeyHex" to publicKeyHex,
      "biometricEnabled" to biometricEnabled,
    )
  }

  private fun generateWrapKey(biometricEnabled: Boolean) {
    val keyGenerator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, ANDROID_KEYSTORE)
    val builder = KeyGenParameterSpec.Builder(
      KEY_ALIAS,
      KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
    )
      .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
      .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
      .setRandomizedEncryptionRequired(true)
      .setUserAuthenticationRequired(biometricEnabled)

    if (biometricEnabled) {
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
        builder.setUserAuthenticationParameters(
          30,
          KeyProperties.AUTH_BIOMETRIC_STRONG or KeyProperties.AUTH_DEVICE_CREDENTIAL,
        )
      } else {
        @Suppress("DEPRECATION")
        builder.setUserAuthenticationValidityDurationSeconds(30)
      }
    }

    keyGenerator.init(builder.build())
    keyGenerator.generateKey()
  }

  private fun wrapKey(): SecretKey {
    val key = keyStore().getKey(KEY_ALIAS, null) as? SecretKey
    if (key != null) {
      return key
    }

    generateWrapKey(prefs.getBoolean(KEY_BIOMETRIC_ENABLED, false))
    return keyStore().getKey(KEY_ALIAS, null) as SecretKey
  }

  private fun keyStore(): KeyStore =
    KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }

  private fun seedToX25519Scalar(seed: ByteArray): ByteArray {
    val scalar = MessageDigest.getInstance("SHA-512").digest(seed).copyOfRange(0, 32)
    scalar[0] = (scalar[0].toInt() and 248).toByte()
    scalar[31] = (scalar[31].toInt() and 127).toByte()
    scalar[31] = (scalar[31].toInt() or 64).toByte()
    return scalar
  }

  private fun hexToBytes(hex: String): ByteArray {
    if (hex.length % 2 != 0) {
      throw CodedException("Invalid hex payload")
    }

    return ByteArray(hex.length / 2) { index ->
      val byteIndex = index * 2
      hex.substring(byteIndex, byteIndex + 2).toInt(16).toByte()
    }
  }

  private fun bytesToHex(bytes: ByteArray): String =
    bytes.joinToString(separator = "") { "%02x".format(it.toInt() and 0xff) }

  private companion object {
    const val ANDROID_KEYSTORE = "AndroidKeyStore"
    const val AES_TRANSFORMATION = "AES/GCM/NoPadding"
    const val KEY_ALIAS = "syncmind-device-identity-wrap"
    const val KEY_BIOMETRIC_ENABLED = "biometricEnabled"
    const val KEY_ENCRYPTED_SEED = "encryptedSeed"
    const val KEY_FINGERPRINT = "fingerprint"
    const val KEY_IV = "seedIv"
    const val KEY_PUBLIC_KEY = "publicKeyHex"
  }
}
