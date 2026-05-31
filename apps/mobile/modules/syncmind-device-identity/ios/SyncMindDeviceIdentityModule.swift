import CryptoKit
import ExpoModulesCore
import Foundation
import Security

public final class SyncMindDeviceIdentityModule: Module {
  private let store = DeviceIdentityStore()

  public func definition() -> ModuleDefinition {
    Name("SyncMindDeviceIdentity")

    AsyncFunction("ensureIdentity") {
      try self.store.ensureIdentity()
    }

    AsyncFunction("getIdentityMeta") {
      self.store.getIdentityMeta()
    }

    AsyncFunction("sign") { (messageBase64: String) in
      try self.store.sign(messageBase64: messageBase64)
    }

    AsyncFunction("deriveX25519") { (peerPubKeyHex: String) in
      try self.store.deriveX25519(peerPubKeyHex: peerPubKeyHex)
    }

    AsyncFunction("setBiometricProtection") { (enabled: Bool) in
      try self.store.setBiometricProtection(enabled: enabled)
    }

    AsyncFunction("resetIdentity") {
      try self.store.resetIdentity()
    }

    AsyncFunction("importLegacyIdentity") { (privateKeyHex: String) in
      try self.store.importLegacyIdentity(privateKeyHex: privateKeyHex)
    }
  }
}

private final class DeviceIdentityStore {
  private let service = "syncmind.mobile.device-identity"
  private let account = "seed"
  private let fingerprintKey = "syncmind.deviceIdentity.fingerprint"
  private let publicKeyKey = "syncmind.deviceIdentity.publicKeyHex"
  private let biometricKey = "syncmind.deviceIdentity.biometricEnabled"
  private let defaults = UserDefaults.standard

  func ensureIdentity() throws -> [String: Any] {
    if let meta = getIdentityMeta() {
      return meta
    }

    if hasSeed() {
      let seed = try readSeed(prompt: "Access SyncMind device identity")
      return try persistMetadata(seed: seed, biometricEnabled: defaults.bool(forKey: biometricKey))
    }

    let seed = try randomSeed()
    try saveSeed(seed, biometricEnabled: false)
    return try persistMetadata(seed: seed, biometricEnabled: false)
  }

  func getIdentityMeta() -> [String: Any]? {
    guard hasSeed(),
      let fingerprint = defaults.string(forKey: fingerprintKey),
      let publicKeyHex = defaults.string(forKey: publicKeyKey)
    else {
      return nil
    }

    return [
      "fingerprint": fingerprint,
      "publicKeyHex": publicKeyHex,
      "biometricEnabled": defaults.bool(forKey: biometricKey)
    ]
  }

  func sign(messageBase64: String) throws -> String {
    guard let message = Data(base64Encoded: messageBase64) else {
      throw IdentityError.invalidBase64
    }

    let seed = try readSeed(prompt: "Sign with SyncMind device identity")
    let privateKey = try Curve25519.Signing.PrivateKey(rawRepresentation: seed)
    let signature = try privateKey.signature(for: message)
    return signature.base64EncodedString()
  }

  func deriveX25519(peerPubKeyHex: String) throws -> String {
    let peerBytes = try Data(hexString: peerPubKeyHex)
    let seed = try readSeed(prompt: "Use SyncMind device identity")
    let scalar = seedToX25519Scalar(seed)
    let privateKey = try Curve25519.KeyAgreement.PrivateKey(rawRepresentation: scalar)
    let publicKey = try Curve25519.KeyAgreement.PublicKey(rawRepresentation: peerBytes)
    let sharedSecret = try privateKey.sharedSecretFromKeyAgreement(with: publicKey)
    let sharedData = sharedSecret.withUnsafeBytes { Data($0) }
    return sharedData.base64EncodedString()
  }

  func setBiometricProtection(enabled: Bool) throws {
    let seed = try readSeed(prompt: "Update SyncMind biometric protection")
    try saveSeed(seed, biometricEnabled: enabled)
    _ = try persistMetadata(seed: seed, biometricEnabled: enabled)
  }

  func resetIdentity() throws {
    let status = SecItemDelete(baseQuery() as CFDictionary)
    if status != errSecSuccess && status != errSecItemNotFound {
      throw IdentityError.keychain(status)
    }

    defaults.removeObject(forKey: fingerprintKey)
    defaults.removeObject(forKey: publicKeyKey)
    defaults.removeObject(forKey: biometricKey)
  }

  func importLegacyIdentity(privateKeyHex: String) throws -> [String: Any] {
    let seed = try Data(hexString: privateKeyHex)
    guard seed.count == 32 else {
      throw IdentityError.invalidSeed
    }

    try saveSeed(seed, biometricEnabled: false)
    return try persistMetadata(seed: seed, biometricEnabled: false)
  }

  private func persistMetadata(seed: Data, biometricEnabled: Bool) throws -> [String: Any] {
    let privateKey = try Curve25519.Signing.PrivateKey(rawRepresentation: seed)
    let publicKeyHex = privateKey.publicKey.rawRepresentation.hexString
    let fingerprint = "sha256:\(Data(SHA256.hash(data: privateKey.publicKey.rawRepresentation)).hexString)"

    defaults.set(fingerprint, forKey: fingerprintKey)
    defaults.set(publicKeyHex, forKey: publicKeyKey)
    defaults.set(biometricEnabled, forKey: biometricKey)

    return [
      "fingerprint": fingerprint,
      "publicKeyHex": publicKeyHex,
      "biometricEnabled": biometricEnabled
    ]
  }

  private func saveSeed(_ seed: Data, biometricEnabled: Bool) throws {
    SecItemDelete(baseQuery() as CFDictionary)

    var query = baseQuery()
    query[kSecValueData as String] = seed

    if biometricEnabled {
      var error: Unmanaged<CFError>?
      guard let accessControl = SecAccessControlCreateWithFlags(
        nil,
        kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
        [.biometryCurrentSet],
        &error
      ) else {
        throw error?.takeRetainedValue() ?? IdentityError.accessControl
      }
      query[kSecAttrAccessControl as String] = accessControl
    } else {
      query[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlockedThisDeviceOnly
    }

    let status = SecItemAdd(query as CFDictionary, nil)
    guard status == errSecSuccess else {
      throw IdentityError.keychain(status)
    }
  }

  private func readSeed(prompt: String) throws -> Data {
    var query = baseQuery()
    query[kSecReturnData as String] = true
    query[kSecMatchLimit as String] = kSecMatchLimitOne
    query[kSecUseOperationPrompt as String] = prompt

    var result: AnyObject?
    let status = SecItemCopyMatching(query as CFDictionary, &result)
    guard status == errSecSuccess else {
      if status == errSecItemNotFound {
        throw IdentityError.noIdentity
      }
      throw IdentityError.keychain(status)
    }

    guard let seed = result as? Data else {
      throw IdentityError.invalidSeed
    }
    return seed
  }

  private func hasSeed() -> Bool {
    var query = baseQuery()
    query[kSecReturnData as String] = false
    query[kSecMatchLimit as String] = kSecMatchLimitOne

    let status = SecItemCopyMatching(query as CFDictionary, nil)
    return status == errSecSuccess || status == errSecInteractionNotAllowed
  }

  private func baseQuery() -> [String: Any] {
    [
      kSecClass as String: kSecClassGenericPassword,
      kSecAttrService as String: service,
      kSecAttrAccount as String: account
    ]
  }

  private func randomSeed() throws -> Data {
    var bytes = [UInt8](repeating: 0, count: 32)
    let count = bytes.count
    let status = bytes.withUnsafeMutableBytes { buffer in
      SecRandomCopyBytes(kSecRandomDefault, count, buffer.baseAddress!)
    }
    guard status == errSecSuccess else {
      throw IdentityError.random(status)
    }
    return Data(bytes)
  }

  private func seedToX25519Scalar(_ seed: Data) -> Data {
    var scalar = Array(Data(SHA512.hash(data: seed)).prefix(32))
    scalar[0] &= 248
    scalar[31] &= 127
    scalar[31] |= 64
    return Data(scalar)
  }
}

private enum IdentityError: LocalizedError {
  case accessControl
  case invalidBase64
  case invalidHex
  case invalidSeed
  case noIdentity
  case keychain(OSStatus)
  case random(OSStatus)

  var errorDescription: String? {
    switch self {
    case .accessControl:
      return "Failed to create biometric access control."
    case .invalidBase64:
      return "Invalid base64 payload."
    case .invalidHex:
      return "Invalid hex payload."
    case .invalidSeed:
      return "Invalid device identity seed."
    case .noIdentity:
      return "Device identity not initialized."
    case .keychain(let status):
      return "Keychain operation failed with status \(status)."
    case .random(let status):
      return "Secure random generation failed with status \(status)."
    }
  }
}

private extension Data {
  init(hexString: String) throws {
    guard hexString.count % 2 == 0 else {
      throw IdentityError.invalidHex
    }

    var bytes = [UInt8]()
    bytes.reserveCapacity(hexString.count / 2)

    var index = hexString.startIndex
    while index < hexString.endIndex {
      let next = hexString.index(index, offsetBy: 2)
      guard let byte = UInt8(hexString[index..<next], radix: 16) else {
        throw IdentityError.invalidHex
      }
      bytes.append(byte)
      index = next
    }

    self = Data(bytes)
  }

  var hexString: String {
    map { String(format: "%02x", $0) }.joined()
  }
}
