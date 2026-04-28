import Cocoa
import InputMethodKit

private let kConnectionName = "org.qbopomofo.inputmethod.QBopomofo_Connection"

private func inputSourceProperty(_ source: TISInputSource, _ key: CFString) -> AnyObject? {
    guard let pointer = TISGetInputSourceProperty(source, key) else {
        return nil
    }
    return Unmanaged<AnyObject>.fromOpaque(pointer).takeUnretainedValue()
}

private func enableAndSelectInputMode(for bundleID: String) {
    guard let sources = TISCreateInputSourceList(nil, true)?.takeRetainedValue() as? [TISInputSource] else {
        NSLog("QBopomofo: Unable to enumerate input sources.")
        return
    }

    let inputMode = sources.first { source in
        let sourceBundleID = inputSourceProperty(source, kTISPropertyBundleID) as? String
        let isSelectable = inputSourceProperty(source, kTISPropertyInputSourceIsSelectCapable) as? Bool ?? false
        return sourceBundleID == bundleID && isSelectable
    }

    guard let inputMode else {
        NSLog("QBopomofo: Registered bundle, but no selectable input source was found.")
        return
    }

    let enableStatus = TISEnableInputSource(inputMode)
    let selectStatus = TISSelectInputSource(inputMode)
    NSLog("QBopomofo: Input source enable status \(enableStatus), select status \(selectStatus).")
}

// Install mode: register input source with macOS
if CommandLine.arguments.count > 1 && CommandLine.arguments[1] == "install" {
    let bundleURL = Bundle.main.bundleURL
    TISRegisterInputSource(bundleURL as CFURL)
    NSLog("QBopomofo: Input source registered from \(bundleURL.path)")
    if let bundleID = Bundle.main.bundleIdentifier {
        enableAndSelectInputMode(for: bundleID)
    }
    exit(0)
}

// Must initialize NSApplication before creating IMKServer
let app = NSApplication.shared

// Register default preferences (auto-learn off by default)
UserDefaults.standard.register(defaults: [
    "org.qbopomofo.disableAutoLearn": true,
    "org.qbopomofo.inputTextFallback": true,
    "org.qbopomofo.shiftMonitorEnabled": true,
])

// Initialize the input method server
guard let bundleID = Bundle.main.bundleIdentifier else {
    NSLog("QBopomofo: Fatal error — no bundle identifier.")
    exit(-1)
}

let server = IMKServer(name: kConnectionName, bundleIdentifier: bundleID)
guard server != nil else {
    NSLog("QBopomofo: Fatal error — cannot initialize IMKServer.")
    exit(-1)
}

NSLog("QBopomofo: Input method server started (build: %@, bundle: %@)", kBuildTimestamp, bundleID)

// Persistent log: date-stamped file, append mode (when env or preference enabled)
let persistentLogEnabled = ProcessInfo.processInfo.environment["QBOPOMOFO_DEBUG"] != nil
    || UserDefaults.standard.bool(forKey: "org.qbopomofo.persistentLog")
if persistentLogEnabled {
    let df = DateFormatter()
    df.dateFormat = "yyyy-MM-dd"
    let dateStr = df.string(from: Date())
    let logPath = "/tmp/qbopomofo-\(dateStr).log"
    if !FileManager.default.fileExists(atPath: logPath) {
        FileManager.default.createFile(atPath: logPath, contents: nil)
    }
    // Symlink /tmp/qbopomofo.log → today's log for `tail -f`
    let link = "/tmp/qbopomofo.log"
    try? FileManager.default.removeItem(atPath: link)
    try? FileManager.default.createSymbolicLink(atPath: link, withDestinationPath: logPath)

    // Redirect stderr to log file so Rust engine debug logs also appear
    freopen(logPath, "a", stderr)

    if let fh = FileHandle(forWritingAtPath: logPath) {
        fh.seekToEndOfFile()
        let msg = "[startup] QBopomofo build: \(kBuildTimestamp)\n"
        fh.write(msg.data(using: .utf8)!)
    }
}
app.run()
