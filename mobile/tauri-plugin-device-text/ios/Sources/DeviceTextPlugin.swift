import SwiftRs
import Tauri
import UIKit
import WebKit

/// What the webview receives. Keys match `TextSettings` in `src/models.rs`.
struct TextSettings: Encodable, Equatable {
  let scale: Double
  let bold: Bool
}

class EventHandlerArgs: Decodable {
  let handler: Channel
}

/// Reports Dynamic Type and Bold Text to the webview.
///
/// WKWebView does not scale page text for the system text size (WebKit closed
/// the request as WONTFIX), so the app reads it here and the web layer applies
/// it. `scale` is the body text size at the user's setting divided by the body
/// size at the default (Large, 17pt): xLarge is 19/17, AX1 is 28/17. It is capped
/// at 2.0, the 200% WCAG asks for, so AX3 and larger all report 2.0.
///
/// The size comes from the web view's trait collection, which carries a per-app
/// size set from Control Center's Text Size control. A change notification
/// carries the new size itself, since the traits may not have caught up yet.
class DeviceTextPlugin: Plugin {
  /// Body text at the default content size category, Large.
  private static let defaultBodySize = 17.0
  private static let maxScale = 2.0

  // Main thread only.
  private weak var webview: WKWebView?
  private var handler: Channel?
  private var lastSent: TextSettings?
  private var observers: [NSObjectProtocol] = []

  @objc public override func load(webview: WKWebView) {
    self.webview = webview
    let center = NotificationCenter.default
    for name in [
      UIContentSizeCategory.didChangeNotification,
      UIAccessibility.boldTextStatusDidChangeNotification,
    ] {
      observers.append(
        center.addObserver(forName: name, object: nil, queue: .main) { [weak self] note in
          let notified = note.userInfo?[UIContentSizeCategory.newValueUserInfoKey]
          self?.sendIfChanged(category: notified as? UIContentSizeCategory)
        })
    }
  }

  /// Called once from Rust at startup with the channel changes go out on.
  @objc public func setEventHandler(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(EventHandlerArgs.self)
    DispatchQueue.main.async {
      self.handler = args.handler
      self.lastSent = self.current()
    }
    invoke.resolve()
  }

  @objc public func get(_ invoke: Invoke) {
    // Commands run on Tauri's IPC queue; UIKit wants the main thread.
    DispatchQueue.main.async {
      invoke.resolve(self.current())
    }
  }

  /// WKWebView has no text scaling of its own to switch off.
  @objc public func follow(_ invoke: Invoke) {
    invoke.resolve()
  }

  /// Both notifications can fire for one change, and Bold Text can post with
  /// nothing different, so only a real change is sent.
  private func sendIfChanged(category: UIContentSizeCategory?) {
    let settings = current(category: category)
    guard settings != lastSent, let handler = handler else { return }
    lastSent = settings
    try? handler.send(settings)
  }

  private func current(category notified: UIContentSizeCategory? = nil) -> TextSettings {
    var category = UIApplication.shared.preferredContentSizeCategory
    if let known = notified ?? webview?.traitCollection.preferredContentSizeCategory,
      known != .unspecified
    {
      category = known
    }
    let traits = UITraitCollection(preferredContentSizeCategory: category)
    let body = Double(UIFont.preferredFont(forTextStyle: .body, compatibleWith: traits).pointSize)
    return TextSettings(
      scale: min(body / DeviceTextPlugin.defaultBodySize, DeviceTextPlugin.maxScale),
      bold: UIAccessibility.isBoldTextEnabled
    )
  }
}

@_cdecl("init_plugin_device_text")
public func initPlugin() -> Plugin {
  return DeviceTextPlugin()
}
