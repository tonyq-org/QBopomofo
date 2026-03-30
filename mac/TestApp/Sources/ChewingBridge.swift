import Foundation
import CChewing

/// Swift wrapper around the chewing C API.
/// Manages a chewing context and provides a clean interface for the test app.
@MainActor
final class ChewingBridge: ObservableObject {

    // MARK: - Published state (drives SwiftUI updates)

    /// Current bopomofo reading (e.g. "ㄊㄞˊ")
    @Published var bopomofoReading: String = ""
    /// Composed text in the pre-edit buffer (e.g. "台北")
    @Published var composingBuffer: String = ""
    /// Combined display: buffer + current reading
    @Published var preEditDisplay: String = ""
    /// Current candidate list
    @Published var candidates: [String] = []
    /// Index of currently selected candidate
    @Published var selectedCandidateIndex: Int = 0
    /// Whether candidate window should be shown
    @Published var showCandidates: Bool = false
    /// Text that has been committed (accumulated output)
    @Published var committedText: String = ""
    /// Log of engine events for debugging
    @Published var debugLog: [String] = []
    /// Current typing mode
    @Published var currentMode: TypingModeSwift = .qBopomofo

    private var ctx: OpaquePointer?

    init() {
        initEngine()
    }

    func cleanup() {
        if let ctx = ctx {
            chewing_delete(ctx)
            self.ctx = nil
        }
    }

    // MARK: - Engine Lifecycle

    func initEngine() {
        // Set dictionary path to data-provider output
        let projectRoot = findProjectRoot()
        let dictPath = projectRoot + "/data-provider/output"
        setenv("CHEWING_PATH", dictPath, 1)

        // Also try test data as fallback
        let testDataPath = projectRoot + "/base/engine/tests/data"
        if !FileManager.default.fileExists(atPath: dictPath + "/word.dat") {
            setenv("CHEWING_PATH", testDataPath, 1)
            log("Using test data from: \(testDataPath)")
        } else {
            log("Using dictionary data from: \(dictPath)")
        }

        ctx = chewing_new()
        guard ctx != nil else {
            log("ERROR: Failed to create chewing context")
            return
        }

        chewing_set_candPerPage(ctx, 9)
        chewing_set_maxChiSymbolLen(ctx, 20)
        chewing_set_spaceAsSelection(ctx, 1)
        chewing_set_escCleanAllBuf(ctx, 1)
        chewing_set_autoShiftCur(ctx, 1)

        log("Engine initialized")
    }

    func reset() {
        guard let ctx = ctx else { return }
        chewing_Reset(ctx)
        committedText = ""
        updateState()
        log("Engine reset")
    }

    // MARK: - Key Handling

    /// Process a key event. Returns true if the key was handled.
    func handleKey(keyCode: UInt16, characters: String, shift: Bool = false) -> Bool {
        guard let ctx = ctx else { return false }

        let handled = processKey(ctx: ctx, keyCode: keyCode, chars: characters, shift: shift)

        if handled {
            // Check for committed text
            if chewing_commit_Check(ctx) != 0 {
                if let commitStr = chewing_commit_String(ctx) {
                    let text = String(cString: commitStr)
                    committedText += text
                    log("Committed: \(text)")
                    chewing_free(commitStr)
                }
            }
            updateState()
        }

        return handled
    }

    private func processKey(ctx: OpaquePointer, keyCode: UInt16, chars: String, shift: Bool) -> Bool {
        switch keyCode {
        case 36: // Return
            chewing_handle_Enter(ctx)
            log("Key: Enter")
            return true
        case 51: // Backspace
            chewing_handle_Backspace(ctx)
            log("Key: Backspace")
            return true
        case 53: // Escape
            chewing_handle_Esc(ctx)
            log("Key: Escape")
            return true
        case 49: // Space
            chewing_handle_Space(ctx)
            log("Key: Space")
            return true
        case 48: // Tab
            chewing_handle_Tab(ctx)
            log("Key: Tab")
            return true
        case 117: // Delete
            chewing_handle_Del(ctx)
            log("Key: Delete")
            return true
        case 123: // Left
            chewing_handle_Left(ctx)
            log("Key: Left")
            return true
        case 124: // Right
            chewing_handle_Right(ctx)
            log("Key: Right")
            return true
        case 125: // Down
            chewing_handle_Down(ctx)
            log("Key: Down")
            return true
        case 126: // Up
            chewing_handle_Up(ctx)
            log("Key: Up")
            return true
        case 116: // Page Up
            chewing_handle_PageUp(ctx)
            return true
        case 121: // Page Down
            chewing_handle_PageDown(ctx)
            return true
        case 115: // Home
            chewing_handle_Home(ctx)
            return true
        case 119: // End
            chewing_handle_End(ctx)
            return true
        default:
            break
        }

        guard let firstChar = chars.first else { return false }
        let charCode = Int32(firstChar.asciiValue ?? 0)
        if charCode > 0 {
            chewing_handle_Default(ctx, charCode)
            log("Key: '\(firstChar)' (code: \(charCode))")
            return true
        }

        return false
    }

    // MARK: - State Update

    private func updateState() {
        guard let ctx = ctx else { return }

        // Bopomofo reading
        if chewing_bopomofo_Check(ctx) != 0,
           let bopoStr = chewing_bopomofo_String(ctx) {
            bopomofoReading = String(cString: bopoStr)
            chewing_free(bopoStr)
        } else {
            bopomofoReading = ""
        }

        // Composing buffer
        if chewing_buffer_Len(ctx) > 0,
           let bufStr = chewing_buffer_String(ctx) {
            composingBuffer = String(cString: bufStr)
            chewing_free(bufStr)
        } else {
            composingBuffer = ""
        }

        // Pre-edit display
        preEditDisplay = composingBuffer + bopomofoReading

        // Candidates
        let totalPage = chewing_cand_TotalPage(ctx)
        if totalPage > 0 {
            var candList: [String] = []
            chewing_cand_Enumerate(ctx)
            while chewing_cand_hasNext(ctx) != 0 {
                if let candStr = chewing_cand_String(ctx) {
                    candList.append(String(cString: candStr))
                    chewing_free(candStr)
                }
            }
            candidates = candList
            showCandidates = !candList.isEmpty
            let currentPage = chewing_cand_CurrentPage(ctx)
            log("Candidates page \(currentPage + 1)/\(totalPage): \(candList.prefix(9).joined(separator: " "))")
        } else {
            candidates = []
            showCandidates = false
        }
    }

    // MARK: - Helpers

    private func log(_ message: String) {
        let timestamp = DateFormatter.localizedString(from: Date(), dateStyle: .none, timeStyle: .medium)
        debugLog.append("[\(timestamp)] \(message)")
        // Keep last 100 entries
        if debugLog.count > 100 {
            debugLog.removeFirst(debugLog.count - 100)
        }
    }

    // MARK: - TypingMode Switching

    func switchMode(_ mode: TypingModeSwift) {
        guard let ctx = ctx else { return }

        // Switch keyboard layout
        chewing_set_KBType(ctx, mode.kbType)

        // Switch conversion engine
        chewing_config_set_int(ctx, "chewing.conversion_engine", mode.conversionEngine)

        currentMode = mode
        log("Mode switched to: \(mode.displayName)")
    }

    // MARK: - Helpers

    private func findProjectRoot() -> String {
        // Walk up from executable to find project root
        var url = URL(fileURLWithPath: #filePath)
        for _ in 0..<10 {
            url = url.deletingLastPathComponent()
            if FileManager.default.fileExists(atPath: url.appendingPathComponent("CLAUDE.md").path) {
                return url.path
            }
        }
        // Fallback: assume we're somewhere in the project
        return FileManager.default.currentDirectoryPath
    }
}

// MARK: - TypingMode Definition (Swift side)

/// Mirrors the Rust TypingMode — bundles layout + conversion engine.
/// KB type values match the C enum in chewing.h.
enum TypingModeSwift: String, CaseIterable, Identifiable {
    case qBopomofo          // QBopomofo 預設模式（所有自訂調校的主要模式）
    case standardBopomofo
    case fuzzyBopomofo
    case hsuBopomofo
    case ibmBopomofo
    case et26Bopomofo
    case hanyuPinyin

    var id: String { rawValue }

    var displayName: String {
        switch self {
        case .qBopomofo: return "Q注音"
        case .standardBopomofo: return "標準注音"
        case .fuzzyBopomofo: return "模糊注音"
        case .hsuBopomofo: return "許氏注音"
        case .ibmBopomofo: return "IBM 注音"
        case .et26Bopomofo: return "倚天26鍵"
        case .hanyuPinyin: return "漢語拼音"
        }
    }

    /// Maps to KB enum in chewing.h (KB_DEFAULT=0, KB_HSU=1, etc.)
    var kbType: Int32 {
        switch self {
        case .qBopomofo: return 0         // KB_DEFAULT（標準鍵盤 + 自訂詞頻）
        case .standardBopomofo: return 0  // KB_DEFAULT
        case .fuzzyBopomofo: return 0     // KB_DEFAULT (layout same, engine differs)
        case .hsuBopomofo: return 1       // KB_HSU
        case .ibmBopomofo: return 2       // KB_IBM
        case .et26Bopomofo: return 5      // KB_ET26
        case .hanyuPinyin: return 9       // KB_HANYU_PINYIN
        }
    }

    /// Maps to conversion engine constants in chewing.h
    var conversionEngine: Int32 {
        switch self {
        case .fuzzyBopomofo: return 2  // FUZZY_CHEWING_CONVERSION_ENGINE
        default: return 1              // CHEWING_CONVERSION_ENGINE
        }
    }
}
