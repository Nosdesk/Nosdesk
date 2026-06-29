import SwiftRs
import Tauri
import UIKit
import WebKit

class SaveArgs: Decodable {
  let token: String
}

struct LoadResult: Encodable {
  let value: String?
}

/// iOS Keychain-backed secure storage for the auth refresh token, via SecItem
/// generic passwords. The token is device-only (never iCloud-synced) with
/// `WhenUnlockedThisDeviceOnly` accessibility: readable without a biometric
/// prompt while the device is unlocked (always true for a foreground refresh),
/// and inaccessible while locked.
class SecureStorePlugin: Plugin {
  private let service = "com.nosdesk.app"
  private let account = "refresh_token"

  private func baseQuery() -> [String: Any] {
    [
      kSecClass as String: kSecClassGenericPassword,
      kSecAttrService as String: service,
      kSecAttrAccount as String: account,
    ]
  }

  @objc public func save(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(SaveArgs.self)
    guard let data = args.token.data(using: .utf8) else {
      invoke.reject("token is not valid UTF-8")
      return
    }
    // Replace any existing item so a rotated token overwrites cleanly.
    SecItemDelete(baseQuery() as CFDictionary)
    var attrs = baseQuery()
    attrs[kSecValueData as String] = data
    attrs[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlockedThisDeviceOnly
    let status = SecItemAdd(attrs as CFDictionary, nil)
    if status != errSecSuccess {
      invoke.reject("keychain save failed: \(status)")
      return
    }
    invoke.resolve()
  }

  @objc public func load(_ invoke: Invoke) throws {
    var query = baseQuery()
    query[kSecReturnData as String] = true
    query[kSecMatchLimit as String] = kSecMatchLimitOne
    var item: CFTypeRef?
    let status = SecItemCopyMatching(query as CFDictionary, &item)
    switch status {
    case errSecSuccess:
      if let data = item as? Data, let token = String(data: data, encoding: .utf8) {
        invoke.resolve(LoadResult(value: token))
      } else {
        invoke.resolve(LoadResult(value: nil))
      }
    case errSecItemNotFound:
      invoke.resolve(LoadResult(value: nil))
    default:
      invoke.reject("keychain load failed: \(status)")
    }
  }

  @objc public func clear(_ invoke: Invoke) throws {
    let status = SecItemDelete(baseQuery() as CFDictionary)
    if status != errSecSuccess && status != errSecItemNotFound {
      invoke.reject("keychain clear failed: \(status)")
      return
    }
    invoke.resolve()
  }
}

@_cdecl("init_plugin_secure_store")
func initPlugin() -> Plugin {
  return SecureStorePlugin()
}
