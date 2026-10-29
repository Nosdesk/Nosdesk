import SwiftRs
import Tauri
import UIKit
import WebKit

class ShareFileArgs: Decodable {
  let path: String
  let mimeType: String
  // Android only (the chooser title). iOS titles the sheet from the file.
  let title: String?
}

struct ShareFileResult: Encodable {
  let completed: Bool
}

/// The iOS share sheet for a file the app has written to its cache.
///
/// `UIActivityViewController` with the file URL offers Save to Files, AirDrop,
/// Mail and, for a PDF, Print, none of which the webview can do itself. The
/// invoke resolves when the sheet closes: `completed` is true when the user
/// ran an activity and false when they dismissed the sheet.
class SharePlugin: Plugin {
  @objc public func shareFile(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(ShareFileArgs.self)
    guard FileManager.default.fileExists(atPath: args.path) else {
      invoke.reject("file not found: \(args.path)")
      return
    }
    let url = URL(fileURLWithPath: args.path)

    DispatchQueue.main.async {
      guard let presenter = self.topViewController() else {
        invoke.reject("no view controller to present the share sheet from")
        return
      }

      let sheet = UIActivityViewController(activityItems: [url], applicationActivities: nil)

      // iPad presents the sheet as a popover and raises an exception without
      // an anchor. Centre it on the presenting view with no arrow. On iPhone
      // the popover controller is nil and this is skipped.
      if let popover = sheet.popoverPresentationController {
        let view: UIView = presenter.view
        popover.sourceView = view
        popover.sourceRect = CGRect(x: view.bounds.midX, y: view.bounds.midY, width: 0, height: 0)
        popover.permittedArrowDirections = []
      }

      // The handler can fire more than once in some activity flows (a share
      // extension that hands back to the sheet). Only the first call answers
      // the invoke.
      var resolved = false
      sheet.completionWithItemsHandler = { _, completed, _, _ in
        if resolved { return }
        resolved = true
        invoke.resolve(ShareFileResult(completed: completed))
      }

      presenter.present(sheet, animated: true)
    }
  }

  /// The view controller currently on top: the webview's controller, or
  /// whatever it is presenting (a sheet already open over it).
  private func topViewController() -> UIViewController? {
    var top = manager.viewController
    while let presented = top?.presentedViewController {
      top = presented
    }
    return top
  }
}

@_cdecl("init_plugin_share")
func initPlugin() -> Plugin {
  return SharePlugin()
}
