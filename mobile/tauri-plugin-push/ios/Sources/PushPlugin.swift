import SwiftRs
import Tauri
import UIKit
import UserNotifications
import WebKit

struct PermissionResult: Encodable {
  let granted: Bool
}

struct TokenResult: Encodable {
  let token: String?
}

/// A tapped notification, surfaced to JS for deep-linking. Mirrors the PII-free
/// APNs payload built in `push_sender.rs` (generic type + entity refs). All
/// `nil` = nothing pending. Keys are camelCase to match `PendingNotification`.
struct NotificationOpened: Encodable {
  let ndType: String?
  let entityType: String?
  let entityId: Int?
  let ticketId: Int?

  /// Build from an APNs `userInfo` dict (custom keys are delivered top-level).
  static func from(userInfo: [AnyHashable: Any]) -> NotificationOpened {
    NotificationOpened(
      ndType: userInfo["nd_type"] as? String,
      entityType: userInfo["entity_type"] as? String,
      entityId: (userInfo["entity_id"] as? NSNumber)?.intValue,
      ticketId: (userInfo["ticket_id"] as? NSNumber)?.intValue
    )
  }
}

/// APNs device-token registration for the mobile app.
///
/// The APNs device token is delivered only to the `UIApplicationDelegate`
/// (`application(_:didRegisterForRemoteNotificationsWithDeviceToken:)`), which
/// the Tauri `Plugin` base class does NOT forward. So on `load()` we swizzle the
/// app delegate's remote-notification callbacks (the same technique Firebase's
/// iOS SDK uses) and stash the token on a shared holder. `getToken` returns the
/// stashed token, waiting briefly if registration is still in flight.
///
/// The token is a routing address, not a secret — it's POSTed to
/// `/api/notifications/devices` by the JS layer with the user's bearer.
class PushPlugin: Plugin, UNUserNotificationCenterDelegate {
  // Shared across the (single) plugin instance + the swizzled delegate IMPs.
  fileprivate static var deviceToken: String?
  fileprivate static var pendingTokenInvokes: [Invoke] = []
  private static var didSwizzle = false
  /// The last notification the user tapped, buffered until the JS layer drains
  /// it via `getPendingNotification`. This buffer is the single source of truth
  /// for deep-linking: the Tauri plugin-event bus does not deliver events to the
  /// webview on iOS, so JS polls this (on app mount + foreground) instead.
  fileprivate static var pendingOpened: NotificationOpened?
  /// `UNUserNotificationCenter.delegate` is a WEAK reference; hold a strong ref
  /// so the delegate can't be deallocated out from under us (which would send
  /// taps to the default handler → app just opens, no routing).
  private static var retainedDelegate: PushPlugin?

  @objc public override func load(webview: WKWebView) {
    PushPlugin.swizzleAppDelegate()
    // Own the notification-center delegate so we get tap + foreground callbacks.
    // Set here (plugin load, early in launch) so a cold-start tap still reaches
    // `didReceive`, which buffers it for `getPendingNotification`.
    PushPlugin.retainedDelegate = self
    UNUserNotificationCenter.current().delegate = self
  }

  /// Return (and clear) the notification the user tapped, so the JS layer can
  /// deep-link. Called on app mount (cold-start tap) and on foreground (warm
  /// tap). All-`nil` when nothing is pending.
  @objc public func getPendingNotification(_ invoke: Invoke) {
    let pending = PushPlugin.pendingOpened ?? NotificationOpened(
      ndType: nil, entityType: nil, entityId: nil, ticketId: nil)
    PushPlugin.pendingOpened = nil
    invoke.resolve(pending)
  }

  // MARK: UNUserNotificationCenterDelegate

  /// Foreground delivery: show the banner/sound even while the app is open,
  /// matching the OS behaviour users expect.
  public func userNotificationCenter(
    _ center: UNUserNotificationCenter,
    willPresent notification: UNNotification,
    withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
  ) {
    completionHandler([.banner, .sound, .badge])
  }

  /// The user tapped a notification. Buffer it; the JS layer drains the buffer
  /// on app mount / foreground and routes (see `getPendingNotification`).
  public func userNotificationCenter(
    _ center: UNUserNotificationCenter,
    didReceive response: UNNotificationResponse,
    withCompletionHandler completionHandler: @escaping () -> Void
  ) {
    PushPlugin.pendingOpened = NotificationOpened.from(
      userInfo: response.notification.request.content.userInfo)
    completionHandler()
  }

  /// Ask for notification permission; on grant, start APNs registration (the
  /// token arrives asynchronously via the swizzled delegate callback).
  @objc public func requestPermission(_ invoke: Invoke) {
    UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .badge, .sound]) {
      granted, _ in
      if granted {
        DispatchQueue.main.async {
          UIApplication.shared.registerForRemoteNotifications()
        }
      }
      invoke.resolve(PermissionResult(granted: granted))
    }
  }

  /// Return the APNs token. If it hasn't arrived yet, (re)trigger registration
  /// and hold the invoke until the delegate delivers it, or resolve `nil` after
  /// a short timeout so the JS caller isn't blocked forever.
  @objc public func getToken(_ invoke: Invoke) {
    if let token = PushPlugin.deviceToken {
      invoke.resolve(TokenResult(token: token))
      return
    }
    PushPlugin.pendingTokenInvokes.append(invoke)
    DispatchQueue.main.async {
      UIApplication.shared.registerForRemoteNotifications()
    }
    DispatchQueue.main.asyncAfter(deadline: .now() + 10) {
      if let idx = PushPlugin.pendingTokenInvokes.firstIndex(where: { $0 === invoke }) {
        PushPlugin.pendingTokenInvokes.remove(at: idx)
        invoke.resolve(TokenResult(token: nil))
      }
    }
  }

  /// Called by the swizzled delegate IMP when APNs returns a token.
  fileprivate static func didReceiveToken(_ token: String) {
    deviceToken = token
    let waiting = pendingTokenInvokes
    pendingTokenInvokes = []
    for invoke in waiting {
      invoke.resolve(TokenResult(token: token))
    }
  }

  /// Called by the swizzled delegate IMP when registration fails — resolve any
  /// waiters with `nil` rather than leaving them to time out.
  fileprivate static func didFailToRegister() {
    let waiting = pendingTokenInvokes
    pendingTokenInvokes = []
    for invoke in waiting {
      invoke.resolve(TokenResult(token: nil))
    }
  }
}

// MARK: - App-delegate swizzling

private var originalDidRegisterImp: IMP?
private var originalDidFailImp: IMP?

extension PushPlugin {
  fileprivate static func swizzleAppDelegate() {
    guard !didSwizzle else { return }
    didSwizzle = true
    guard let delegate = UIApplication.shared.delegate else { return }
    let cls: AnyClass = type(of: delegate)

    // application(_:didRegisterForRemoteNotificationsWithDeviceToken:)
    let registerSel = #selector(
      UIApplicationDelegate.application(_:didRegisterForRemoteNotificationsWithDeviceToken:))
    let registerBlock: @convention(block) (AnyObject, UIApplication, Data) -> Void = {
      _self, application, deviceToken in
      let token = deviceToken.map { String(format: "%02x", $0) }.joined()
      PushPlugin.didReceiveToken(token)
      if let original = originalDidRegisterImp {
        typealias Fn = @convention(c) (AnyObject, Selector, UIApplication, Data) -> Void
        unsafeBitCast(original, to: Fn.self)(_self, registerSel, application, deviceToken)
      }
    }
    let registerImp = imp_implementationWithBlock(registerBlock)
    if let existing = class_getInstanceMethod(cls, registerSel) {
      originalDidRegisterImp = method_setImplementation(existing, registerImp)
    } else {
      _ = class_addMethod(cls, registerSel, registerImp, "v@:@@")
    }

    // application(_:didFailToRegisterForRemoteNotificationsWithError:)
    let failSel = #selector(
      UIApplicationDelegate.application(_:didFailToRegisterForRemoteNotificationsWithError:))
    let failBlock: @convention(block) (AnyObject, UIApplication, Error) -> Void = {
      _self, application, error in
      PushPlugin.didFailToRegister()
      if let original = originalDidFailImp {
        typealias Fn = @convention(c) (AnyObject, Selector, UIApplication, Error) -> Void
        unsafeBitCast(original, to: Fn.self)(_self, failSel, application, error)
      }
    }
    let failImp = imp_implementationWithBlock(failBlock)
    if let existing = class_getInstanceMethod(cls, failSel) {
      originalDidFailImp = method_setImplementation(existing, failImp)
    } else {
      _ = class_addMethod(cls, failSel, failImp, "v@:@@")
    }
  }
}

@_cdecl("init_plugin_push")
func initPlugin() -> Plugin {
  return PushPlugin()
}
