import Cocoa
import InputMethodKit

private let kConnectionName = "QBopomofo_Connection"

// Install mode: register input source with macOS
if CommandLine.arguments.count > 1 && CommandLine.arguments[1] == "install" {
    let bundleURL = Bundle.main.bundleURL
    TISRegisterInputSource(bundleURL as CFURL)
    NSLog("QBopomofo: Input source registered from \(bundleURL.path)")
    exit(0)
}

// Initialize the input method server
guard let bundleID = Bundle.main.bundleIdentifier,
      let server = IMKServer(name: kConnectionName, bundleIdentifier: bundleID)
else {
    NSLog("QBopomofo: Fatal error — cannot initialize IMKServer.")
    exit(-1)
}

// Keep server reference alive
_ = server

NSLog("QBopomofo: Input method server started.")
NSApp.run()
