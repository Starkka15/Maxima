import Cocoa
import Foundation

// MaximaHelper — silent background agent for macOS/CrossOver login flow.
//
// Registers as the handler for the qrc:// URL scheme. When EA's OAuth
// flow redirects to qrc:// (which macOS browsers can open but Wine cannot),
// this app intercepts the URL and forwards it to Maxima's local TCP listener.

class AppDelegate: NSObject, NSApplicationDelegate {
    private let maximaPort = 31033
    private var pendingTask: URLSessionDataTask?

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSAppleEventManager.shared().setEventHandler(
            self,
            andSelector: #selector(handleGetURL(_:withReply:)),
            forEventClass: AEEventClass(kInternetEventClass),
            andEventID: AEEventID(kAEGetURL)
        )
    }

    @objc func handleGetURL(_ event: NSAppleEventDescriptor, withReply reply: NSAppleEventDescriptor) {
        guard
            let rawURL = event.paramDescriptor(forKeyword: keyDirectObject)?.stringValue,
            let url = URL(string: rawURL),
            url.scheme == "qrc"
        else { return }

        forward(url)
    }

    private func forward(_ url: URL) {
        guard let components = URLComponents(url: url, resolvingAgainstBaseURL: false),
              let query = components.query,
              let target = URL(string: "http://127.0.0.1:\(maximaPort)/auth?\(query)")
        else { return }

        var request = URLRequest(url: target, timeoutInterval: 5)
        request.httpMethod = "GET"

        pendingTask = URLSession.shared.dataTask(with: request) { [weak self] _, response, error in
            if let error = error {
                os_log("MaximaHelper: failed to forward qrc:// URL — %{public}@", log: .default, type: .error, error.localizedDescription)
            }
            DispatchQueue.main.async {
                self?.pendingTask = nil
                NSApp.terminate(nil)
            }
        }
        pendingTask?.resume()
    }
}

import os.log

let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.run()
