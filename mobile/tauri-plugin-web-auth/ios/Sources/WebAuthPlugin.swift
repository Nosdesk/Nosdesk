import SwiftRs
import Tauri
import UIKit
import WebKit
import AuthenticationServices

class AuthenticateArgs: Decodable {
  let url: String
  let callbackScheme: String
}

class WebAuthPlugin: Plugin {
  @objc public func authenticate(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(AuthenticateArgs.self)
    guard let url = URL(string: args.url) else {
      invoke.reject("Invalid URL")
      return
    }

    let session = ASWebAuthenticationSession(
      url: url,
      callbackURLScheme: args.callbackScheme
    ) { callbackURL, error in
      if let error = error {
        invoke.reject(error.localizedDescription)
        return
      }
      
      invoke.resolve([
        "callbackUrl": callbackURL?.absoluteString ?? ""
      ])
    }

    // To present the session, get the topmost view controller
    if #available(iOS 13.0, *) {
      session.presentationContextProvider = self
    }
    session.start()
  }
}

// For iOS 13+, provide a presentation context for ASWebAuthenticationSession
extension WebAuthPlugin: ASWebAuthenticationPresentationContextProviding {
  @available(iOS 13.0, *)
  func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor {
    return UIApplication.shared.windows.first { $0.isKeyWindow } ?? ASPresentationAnchor()
  }
}

@_cdecl("init_plugin_web_auth")
public func initPlugin() -> Plugin {
  return WebAuthPlugin()
}
