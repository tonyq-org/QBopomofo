import Foundation
import CChewing

/// Swift wrapper around chewing C API + QBopomofo composing session.
/// All composing logic (Shift SmartToggle, mixed segments, commit order)
/// lives in the Rust engine via qb_composing_* C API.
/// This class is purely a thin UI bridge.
@MainActor
final class ChewingBridge: ObservableObject {

    // MARK: - Timing
    var lastKeyTime: CFAbsoluteTime = 0

    // MARK: - Published state (drives SwiftUI updates)

    @Published var bopomofoReading: String = ""
    @Published var composingBuffer: String = ""
    @Published var preEditDisplay: String = ""
    @Published var candidates: [String] = []
    @Published var selectedCandidateIndex: Int = 0
    @Published var showCandidates: Bool = false
    @Published var candidateCurrentPage: Int = 0
    @Published var candidateTotalPages: Int = 0
    @Published var committedText: String = ""
    @Published var isEnglishMode: Bool = false
    @Published var debugLog: String = ""
    private var debugLogPending: String = ""
    private var debugLogLineCount: Int = 0
    private var debugLogFlushScheduled: Bool = false
    private var isAutoFlushing: Bool = false
    @Published var currentMode: TypingModeSwift = .qBopomofo
    @Published var shiftBehavior: ShiftBehaviorSwift = .smartToggle
    @Published var capsLockBehavior: CapsLockBehaviorSwift = .none
    @Published var candidatesPerPage: Int = 9
    @Published var spaceAsSelection: Bool = true
    @Published var escClearAll: Bool = true
    @Published var autoLearn: Bool = true

    /// Chewing engine context (C API)
    private var ctx: OpaquePointer?
    /// QBopomofo composing session (Rust C API) — manages Shift, mixed segments
    private var session: OpaquePointer?

    init() {
        initEngine()
    }

    func cleanup() {
        if let ctx = ctx { chewing_delete(ctx); self.ctx = nil }
        if let session = session { qb_composing_delete(session); self.session = nil }
    }

    // MARK: - Engine Lifecycle

    func initEngine() {
        let projectRoot = findProjectRoot()
        let dictPath = projectRoot + "/data-provider/output"
        let testDataPath = projectRoot + "/base/engine/tests/data"
        if !FileManager.default.fileExists(atPath: dictPath + "/word.dat") {
            setenv("CHEWING_PATH", testDataPath, 1)
            log("Using test data from: \(testDataPath)")
        } else {
            setenv("CHEWING_PATH", dictPath, 1)
            log("Using dictionary data from: \(dictPath)")
        }

        ctx = chewing_new()
        guard ctx != nil else {
            log("ERROR: Failed to create chewing context")
            return
        }

        session = qb_composing_new()
        guard session != nil else {
            log("ERROR: Failed to create composing session")
            return
        }

        chewing_set_candPerPage(ctx, 9)
        chewing_set_maxChiSymbolLen(ctx, 20)
        chewing_set_spaceAsSelection(ctx, 1)
        chewing_set_escCleanAllBuf(ctx, 1)
        chewing_set_autoShiftCur(ctx, 1)

        // Apply default shift behavior
        qb_composing_set_shift_behavior(session, 1) // SmartToggle

        log("Engine initialized")
    }

    func reset() {
        guard let ctx = ctx else { return }
        chewing_Reset(ctx)
        qb_composing_clear(session)
        committedText = ""
        isEnglishMode = false
        updateState()
        log("Engine reset")
    }

    // MARK: - Shift Handling (delegates to Rust)

    func handleShiftToggle(isShiftDown: Bool) {
        guard let ctx = ctx, let session = session else { return }

        // Get current Chinese buffer to pass to Rust
        let chineseBuf = getChewingBufferString()
        let changed = chineseBuf.withCString { cStr in
            qb_composing_handle_shift(session, isShiftDown ? 1 : 0, cStr)
        }

        isEnglishMode = qb_composing_is_english(session) != 0

        if isShiftDown {
            log("Shift ↓")
        } else if changed != 0 {
            log("Shift ↑ → \(isEnglishMode ? "英文" : "中文") mode")
        } else {
            log("Shift ↑")
        }

        updateState()
    }

    // MARK: - Key Handling

    func handleKey(keyCode: UInt16, characters: String, shift: Bool = false) -> Bool {
        lastKeyTime = CFAbsoluteTimeGetCurrent()
        defer {
            let elapsed = (CFAbsoluteTimeGetCurrent() - lastKeyTime) * 1000
            log("⏱ engine \(String(format: "%.2f", elapsed))ms")
        }
        guard let ctx = ctx, let session = session else { return false }

        let rustEnglish = qb_composing_is_english(session) != 0
        let hasMixed = qb_composing_has_mixed_content(session) != 0
        if rustEnglish || hasMixed {
            log("DEBUG mode=\(rustEnglish ? "EN" : "CH") mixed=\(hasMixed) swiftEN=\(isEnglishMode)")
        }

        // Shift held + typing → English (letters only; punctuation falls through to engine)
        if shift && qb_composing_is_shift_held(session) != 0 {
            if let ch = characters.first, ch.isASCII, ch.isLetter {
                let chinBuf = getChewingBufferString()
                let directCommit = chinBuf.withCString { cStr in
                    qb_composing_type_english(session, UInt8(ch.asciiValue ?? 0), cStr)
                }
                isEnglishMode = qb_composing_is_english(session) != 0
                if directCommit != 0 {
                    committedText += String(ch)
                    log("Key (temp English, direct): '\(ch)'")
                } else {
                    log("Key (temp English → 組字區): '\(ch)'")
                }
                updateState()
                return true
            }
            // Non-letter key while Shift held: mark used so release won't toggle mode
            qb_composing_mark_shift_used(session)
        }

        // English mode — type into session
        if qb_composing_is_english(session) != 0 {
            if keyCode == 53 { // Escape
                qb_composing_clear(session)
                chewing_handle_Esc(ctx)
                isEnglishMode = false
                log("Key: Escape (清除組字區)")
                updateState()
                return true
            }
            if keyCode == 36 { // Enter — commit all
                commitAll()
                log("Key: Enter (commit all)")
                return true
            }
            if keyCode == 51 { // Backspace
                if qb_composing_backspace_english(session) != 0 {
                    log("Key: Backspace (English)")
                    updateState()
                    return true
                }
            }
            // Space in English mode
            if keyCode == 49 {
                let chinBuf = getChewingBufferString()
                let directCommit = chinBuf.withCString { cStr in
                    qb_composing_type_english(session, UInt8(Character(" ").asciiValue!), cStr)
                }
                if directCommit != 0 {
                    committedText += " "
                    log("Key (English, direct): Space")
                } else {
                    log("Key (English → 組字區): Space")
                }
                updateState()
                return true
            }
            if let ch = characters.first, ch.isASCII, !ch.isNewline {
                let chinBuf = getChewingBufferString()
                let directCommit = chinBuf.withCString { cStr in
                    qb_composing_type_english(session, UInt8(ch.asciiValue ?? 0), cStr)
                }
                if directCommit != 0 {
                    committedText += String(ch)
                    log("Key (English, direct): '\(ch)'")
                } else {
                    log("Key (English → 組字區): '\(ch)'")
                }
                updateState()
                return true
            }
        }

        // Backspace with mixed content — try deleting from session first
        if keyCode == 51 && qb_composing_has_mixed_content(session) != 0 {
            if qb_composing_backspace_english(session) != 0 {
                log("Key: Backspace (deleted English from segment)")
                updateState()
                return true
            }
        }

        // Enter with mixed content
        if keyCode == 36 && qb_composing_has_mixed_content(session) != 0 {
            commitAll()
            log("Key: Enter (commit all: Chinese + English)")
            return true
        }

        // Escape with mixed content
        if keyCode == 53 && qb_composing_has_mixed_content(session) != 0 {
            qb_composing_clear(session)
            chewing_handle_Esc(ctx)
            isEnglishMode = false
            log("Key: Escape (清除組字區)")
            updateState()
            return true
        }

        // Candidate mode — intercept navigation keys
        if showCandidates && chewing_cand_TotalPage(ctx) > 0 {
            switch keyCode {
            case 125: // Down — next candidate
                selectedCandidateIndex = min(selectedCandidateIndex + 1, candidates.count - 1)
                log("Candidate ↓ → \(selectedCandidateIndex)")
                return true
            case 126: // Up — previous candidate
                selectedCandidateIndex = max(selectedCandidateIndex - 1, 0)
                log("Candidate ↑ → \(selectedCandidateIndex)")
                return true
            case 124: // Right — next page
                chewing_cand_list_next(ctx)
                log("Candidate page →")
                selectedCandidateIndex = 0
                updateState()
                return true
            case 123: // Left — previous page
                chewing_cand_list_prev(ctx)
                log("Candidate page ←")
                selectedCandidateIndex = 0
                updateState()
                return true
            case 36: // Enter — select current candidate
                let pageOffset = chewing_cand_CurrentPage(ctx) * Int32(candidatesPerPage)
                chewing_cand_choose_by_index(ctx, pageOffset + Int32(selectedCandidateIndex))
                log("Candidate selected: \(selectedCandidateIndex)")
                selectedCandidateIndex = 0
                updateState()
                return true
            case 53: // Escape — close candidates
                chewing_cand_close(ctx)
                log("Candidate cancelled")
                selectedCandidateIndex = 0
                updateState()
                return true
            case 49: // Space — select first candidate (same as Enter for index 0)
                let pageOff = chewing_cand_CurrentPage(ctx) * Int32(candidatesPerPage)
                chewing_cand_choose_by_index(ctx, pageOff + Int32(selectedCandidateIndex))
                log("Candidate selected (space): \(selectedCandidateIndex)")
                selectedCandidateIndex = 0
                updateState()
                return true
            default:
                // Number keys 1-9 select directly
                if let ch = characters.first, ch >= "1" && ch <= "9" {
                    let idx = Int(String(ch))! - 1
                    if idx < candidates.count {
                        let pageOff = chewing_cand_CurrentPage(ctx) * Int32(candidatesPerPage)
                        chewing_cand_choose_by_index(ctx, pageOff + Int32(idx))
                        log("Candidate selected (#\(idx + 1))")
                        selectedCandidateIndex = 0
                        updateState()
                        return true
                    }
                }
                break
            }
        }

        // Chinese mode: space with no buffer → output space directly
        if keyCode == 49 && chewing_buffer_Len(ctx) == 0 && chewing_bopomofo_Check(ctx) == 0 {
            committedText += " "
            log("Key: Space (direct, no buffer)")
            return true
        }

        // Chinese mode — send to chewing engine
        let handled = processKey(ctx: ctx, keyCode: keyCode, chars: characters)

        if handled {
            if chewing_commit_Check(ctx) != 0 {
                if let commitStr = chewing_commit_String(ctx) {
                    let text = String(cString: commitStr)
                    committedText += text
                    log("Committed: \(text)")
                    chewing_free(commitStr)
                }
                _ = chewing_ack(ctx)
            }
            updateState()
        }

        return handled
    }

    private func processKey(ctx: OpaquePointer, keyCode: UInt16, chars: String) -> Bool {
        switch keyCode {
        case 36:
            chewing_handle_Enter(ctx)
            log("Key: Enter")
            return true
        case 51:
            chewing_handle_Backspace(ctx)
            log("Key: Backspace")
            return true
        case 53:
            chewing_handle_Esc(ctx)
            isEnglishMode = false
            log("Key: Escape (清除組字區)")
            return true
        case 49:
            chewing_handle_Space(ctx)
            log("Key: Space")
            return true
        case 48:
            chewing_handle_Tab(ctx)
            log("Key: Tab")
            return true
        case 117:
            chewing_handle_Del(ctx)
            log("Key: Delete")
            return true
        case 123:
            chewing_handle_Left(ctx)
            log("Key: Left")
            return true
        case 124:
            chewing_handle_Right(ctx)
            log("Key: Right")
            return true
        case 125: // Down — open candidate window
            if chewing_buffer_Len(ctx) > 0 {
                chewing_cand_open(ctx)
                selectedCandidateIndex = 0
                log("Key: Down (open candidates)")
            } else {
                chewing_handle_Down(ctx)
                log("Key: Down")
            }
            return true
        case 126:
            chewing_handle_Up(ctx)
            log("Key: Up")
            return true
        case 116:
            chewing_handle_PageUp(ctx)
            return true
        case 121:
            chewing_handle_PageDown(ctx)
            return true
        case 115:
            chewing_handle_Home(ctx)
            return true
        case 119:
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

    // MARK: - Commit

    private func commitAll() {
        guard let ctx = ctx, let session = session else { return }

        // Get Chinese text from chewing engine
        var finalChinese = ""
        if chewing_buffer_Len(ctx) > 0 {
            chewing_handle_Enter(ctx)
            if chewing_commit_Check(ctx) != 0 {
                if let commitStr = chewing_commit_String(ctx) {
                    finalChinese = String(cString: commitStr)
                    chewing_free(commitStr)
                }
                _ = chewing_ack(ctx)
            }
        }

        // Let Rust session build the correctly-ordered result
        let result = finalChinese.withCString { cStr -> String in
            if let resultPtr = qb_composing_commit_all(session, cStr) {
                let s = String(cString: resultPtr)
                chewing_free(resultPtr)
                return s
            }
            return finalChinese
        }

        committedText += result
        isEnglishMode = qb_composing_is_english(session) != 0
        log("Commit all: \(result)")
        updateState()
    }

    // MARK: - State Update

    private func updateState() {
        guard let ctx = ctx, let session = session else { return }

        // Bopomofo reading
        let newBopo: String
        if chewing_bopomofo_Check(ctx) != 0,
           let bopoStr = chewing_bopomofo_String(ctx) {
            newBopo = String(cString: bopoStr)
            chewing_free(bopoStr)
        } else {
            newBopo = ""
        }
        if bopomofoReading != newBopo { bopomofoReading = newBopo }

        // Composing buffer (internal, not directly displayed)
        let newBuf: String
        if chewing_buffer_Len(ctx) > 0,
           let bufStr = chewing_buffer_String(ctx) {
            newBuf = String(cString: bufStr)
            chewing_free(bufStr)
        } else {
            newBuf = ""
        }
        if composingBuffer != newBuf { composingBuffer = newBuf }

        // Build full display via Rust session (handles mixed segment ordering)
        let newDisplay = newBuf.withCString { chinBuf in
            newBopo.withCString { bopoBuf in
                if let displayPtr = qb_composing_build_display(session, chinBuf, bopoBuf) {
                    let s = String(cString: displayPtr)
                    chewing_free(displayPtr)
                    return s
                }
                return newBuf + newBopo
            }
        }
        if preEditDisplay != newDisplay { preEditDisplay = newDisplay }

        // English mode sync
        let newEnglish = qb_composing_is_english(session) != 0
        if isEnglishMode != newEnglish { isEnglishMode = newEnglish }

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
            if candidates != candList { candidates = candList }
            if !showCandidates { showCandidates = true }
            let currentPage = chewing_cand_CurrentPage(ctx)
            if candidateCurrentPage != Int(currentPage) { candidateCurrentPage = Int(currentPage) }
            if candidateTotalPages != Int(totalPage) { candidateTotalPages = Int(totalPage) }
            log("Candidates page \(currentPage + 1)/\(totalPage): \(candList.prefix(candidatesPerPage).joined(separator: " "))")
        } else {
            if !candidates.isEmpty { candidates = [] }
            if showCandidates { showCandidates = false }
            if candidateTotalPages != 0 { candidateTotalPages = 0 }
        }

        // Auto-flush: composing display > 20 chars → commit
        if !isAutoFlushing && newDisplay.count > 20 {
            isAutoFlushing = true
            let hasChinese = !newBuf.isEmpty
            if !hasChinese {
                // Pure English — flush all
                commitAll()
                log("Auto-flush (純英文, \(newDisplay.count) chars)")
            } else {
                // Mixed — flush all
                commitAll()
                log("Auto-flush (中英混合, \(newDisplay.count) chars)")
            }
            isAutoFlushing = false
        }
    }

    // MARK: - Helpers

    private func getChewingBufferString() -> String {
        guard let ctx = ctx else { return "" }
        if chewing_buffer_Len(ctx) > 0,
           let bufStr = chewing_buffer_String(ctx) {
            let s = String(cString: bufStr)
            chewing_free(bufStr)
            return s
        }
        return ""
    }

    func clearLog() {
        debugLog = ""
        debugLogPending = ""
        debugLogLineCount = 0
    }

    func logRender(_ elapsedMs: Double) {
        log("⏱ render \(String(format: "%.2f", elapsedMs))ms")
    }

    private func log(_ message: String) {
        let timestamp = DateFormatter.localizedString(from: Date(), dateStyle: .none, timeStyle: .medium)
        let line = "[\(timestamp)] \(message)"
        if debugLogPending.isEmpty {
            debugLogPending = line
        } else {
            debugLogPending += "\n" + line
        }
        debugLogLineCount += 1
        // Flush on next run loop — batches multiple log() calls into one @Published update
        if !debugLogFlushScheduled {
            debugLogFlushScheduled = true
            DispatchQueue.main.async { [weak self] in
                self?.flushLog()
            }
        }
    }

    private func flushLog() {
        debugLogFlushScheduled = false
        guard !debugLogPending.isEmpty else { return }
        if debugLog.isEmpty {
            debugLog = debugLogPending
        } else {
            debugLog += "\n" + debugLogPending
        }
        debugLogPending = ""
        // Trim to 200 lines
        while debugLogLineCount > 200 {
            if let idx = debugLog.firstIndex(of: "\n") {
                debugLog = String(debugLog[debugLog.index(after: idx)...])
                debugLogLineCount -= 1
            } else {
                break
            }
        }
    }

    // MARK: - TypingMode Switching

    func switchMode(_ mode: TypingModeSwift) {
        guard let ctx = ctx, let session = session else { return }
        chewing_set_KBType(ctx, mode.kbType)
        chewing_config_set_int(ctx, "chewing.conversion_engine", mode.conversionEngine)
        shiftBehavior = mode.defaultShiftBehavior
        capsLockBehavior = mode.defaultCapsLockBehavior

        // Sync shift behavior to Rust session
        let shiftVal: Int32 = switch shiftBehavior {
        case .none: 0
        case .smartToggle: 1
        case .toggleOnly: 2
        }
        qb_composing_set_shift_behavior(session, shiftVal)

        currentMode = mode
        applyPreferences()
        log("Mode switched to: \(mode.displayName)")
    }

    func applyPreferences() {
        guard let ctx = ctx else { return }
        chewing_set_candPerPage(ctx, Int32(candidatesPerPage))
        chewing_set_spaceAsSelection(ctx, spaceAsSelection ? 1 : 0)
        chewing_set_escCleanAllBuf(ctx, escClearAll ? 1 : 0)
        chewing_set_autoLearn(ctx, autoLearn ? 0 : 1)
        log("Preferences applied: shift=\(shiftBehavior), capsLock=\(capsLockBehavior), cand/page=\(candidatesPerPage)")
    }

    private func findProjectRoot() -> String {
        var url = URL(fileURLWithPath: #filePath)
        for _ in 0..<10 {
            url = url.deletingLastPathComponent()
            if FileManager.default.fileExists(atPath: url.appendingPathComponent("CLAUDE.md").path) {
                return url.path
            }
        }
        return FileManager.default.currentDirectoryPath
    }
}

// MARK: - TypingMode Definition (Swift side)

enum TypingModeSwift: String, CaseIterable, Identifiable {
    case qBopomofo
    case standardBopomofo
    case fuzzyBopomofo
    case abbreviatedBopomofo
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
        case .abbreviatedBopomofo: return "簡拼注音"
        case .hsuBopomofo: return "許氏注音"
        case .ibmBopomofo: return "IBM 注音"
        case .et26Bopomofo: return "倚天26鍵"
        case .hanyuPinyin: return "漢語拼音"
        }
    }

    var kbType: Int32 {
        switch self {
        case .qBopomofo: return 0
        case .standardBopomofo: return 0
        case .fuzzyBopomofo: return 0
        case .abbreviatedBopomofo: return 0
        case .hsuBopomofo: return 1
        case .ibmBopomofo: return 2
        case .et26Bopomofo: return 5
        case .hanyuPinyin: return 9
        }
    }

    var conversionEngine: Int32 {
        switch self {
        case .fuzzyBopomofo: return 2
        case .abbreviatedBopomofo: return 3
        default: return 1
        }
    }

    var defaultShiftBehavior: ShiftBehaviorSwift {
        switch self {
        case .qBopomofo, .abbreviatedBopomofo: return .smartToggle
        default: return .none
        }
    }

    var defaultCapsLockBehavior: CapsLockBehaviorSwift {
        return .none
    }
}

// MARK: - Preference Enums

enum ShiftBehaviorSwift: String, CaseIterable, Identifiable {
    case none
    case smartToggle
    case toggleOnly

    var id: String { rawValue }

    var displayName: String {
        switch self {
        case .none: return "不處理"
        case .smartToggle: return "智慧切換（短按切換，長按暫時英文）"
        case .toggleOnly: return "僅切換中/英"
        }
    }
}

enum CapsLockBehaviorSwift: String, CaseIterable, Identifiable {
    case none
    case toggleChineseEnglish
    case toggleFullHalfWidth

    var id: String { rawValue }

    var displayName: String {
        switch self {
        case .none: return "不處理"
        case .toggleChineseEnglish: return "切換中/英"
        case .toggleFullHalfWidth: return "切換全/半形"
        }
    }
}
