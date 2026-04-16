import Cocoa
import InputMethodKit

private let kConnectionName = "org.qbopomofo.inputmethod.QBopomofo_Connection"

// Install mode: register input source with macOS
if CommandLine.arguments.count > 1 && CommandLine.arguments[1] == "install" {
    let bundleURL = Bundle.main.bundleURL
    TISRegisterInputSource(bundleURL as CFURL)
    NSLog("QBopomofo: Input source registered from \(bundleURL.path)")
    exit(0)
}

// Must initialize NSApplication before creating IMKServer
let app = NSApplication.shared

// Register default preferences (auto-learn off by default)
UserDefaults.standard.register(defaults: [
    "org.qbopomofo.disableAutoLearn": true,
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
