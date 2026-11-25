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
/// at 2.0, the 200% WCAG asks for; pinch zoom covers the rest.
///
/// The size comes from `UIApplication.preferredContentSizeCategory`, which
/// includes a per-app size set from Control Center's Text Size control.
class DeviceTextPlugin: Plugin {
  /// Body text at the default content size category, Large.
  private static let defaultBodySize = 17.0
  private static let maxScale = 2.0

  // Main thread only.
  private var handler: Channel?
  private var lastSent: TextSettings?
  private var observers: [NSObjectProtocol] = []

  @objc public override func load(webview: WKWebView) {
    let center = NotificationCenter.default
    for name in [
      UIContentSizeCategory.didChangeNotification,
      UIAccessibility.boldTextStatusDidChangeNotification,
    ] {
      observers.append(
        center.addObserver(forName: name, object: nil, queue: .main) { [weak self] _ in
          self?.sendIfChanged()
        })
    }
  }

  /// Called once from Rust at startup with the channel changes go out on.
  @objc public func setEventHandler(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(EventHandlerArgs.self)
    DispatchQueue.main.async {
      self.handler = args.handler
      self.lastSent = DeviceTextPlugin.current()
    }
    invoke.resolve()
  }

  @objc public func get(_ invoke: Invoke) {
    // Commands run on Tauri's IPC queue; UIKit wants the main thread.
    DispatchQueue.main.async {
      invoke.resolve(DeviceTextPlugin.current())
    }
  }

  /// Both notifications can fire for one change, and Bold Text can post with
  /// nothing different, so only a real change is sent.
  private func sendIfChanged() {
    let settings = DeviceTextPlugin.current()
    guard settings != lastSent, let handler = handler else { return }
    lastSent = settings
    try? handler.send(settings)
  }

  private static func current() -> TextSettings {
    let category = UIApplication.shared.preferredContentSizeCategory
    let traits = UITraitCollection(preferredContentSizeCategory: category)
    let body = Double(UIFont.preferredFont(forTextStyle: .body, compatibleWith: traits).pointSize)
    return TextSettings(
      scale: min(body / defaultBodySize, maxScale),
      bold: UIAccessibility.isBoldTextEnabled
    )
  }
}

@_cdecl("init_plugin_device_text")
public func initPlugin() -> Plugin {
  return DeviceTextPlugin()
}
