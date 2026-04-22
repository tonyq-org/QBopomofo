import Cocoa
@preconcurrency import InputMethodKit
import CChewing

/// Debug mode: env var OR user preference
private let kDebugMode = ProcessInfo.processInfo.environment["QBOPOMOFO_DEBUG"] != nil
private var kPersistentLog: Bool {
    kDebugMode || UserDefaults.standard.bool(forKey: "org.qbopomofo.persistentLog")
}

/// Persistent log: writes to date-stamped file in /tmp/ when enabled
nonisolated(unsafe) private var persistentLogHandle: FileHandle? = {
    let df = DateFormatter()
    df.dateFormat = "yyyy-MM-dd"
    let dateStr = df.string(from: Date())
    let path = "/tmp/qbopomofo-\(dateStr).log"
    if !FileManager.default.fileExists(atPath: path) {
        FileManager.default.createFile(atPath: path, contents: nil)
    }
    let handle = FileHandle(forWritingAtPath: path)
    handle?.seekToEndOfFile()
    return handle
}()

private func dbg(_ msg: String) {
    guard kPersistentLog else { return }
    let ts = DateFormatter.localizedString(from: Date(), dateStyle: .none, timeStyle: .medium)
    let line = "[\(ts)] \(msg)\n"
    if let data = line.data(using: .utf8) {
        persistentLogHandle?.seekToEndOfFile()
        persistentLogHandle?.write(data)
    }
}

// Correction log: always records candidate corrections for phrase tuning
nonisolated(unsafe) private var correctionLogHandle: FileHandle? = {
    let path = "/tmp/qbopomofo-corrections.log"
    if !FileManager.default.fileExists(atPath: path) {
        FileManager.default.createFile(atPath: path, contents: nil)
    }
    let handle = FileHandle(forWritingAtPath: path)
    handle?.seekToEndOfFile()
    return handle
}()

private func logCorrection(_ entry: String) {
    guard kPersistentLog, let handle = correctionLogHandle else { return }
    let ts = DateFormatter.localizedString(from: Date(), dateStyle: .short, timeStyle: .medium)
    let line = "[\(ts)] \(entry)\n"
    if let data = line.data(using: .utf8) {
        handle.seekToEndOfFile()
        handle.write(data)
    }
}

/// QBopomofo 的核心輸入控制器
/// 負責處理按鍵事件、與 libchewing 引擎互動、管理輸入狀態
/// 組字邏輯（Shift SmartToggle、中英混排）委託給 Rust ComposingSession (qb_composing_*)
@objc(QBopomofoInputController)
class QBopomofoInputController: IMKInputController {

    // MARK: - Properties

    private var chewingContext: OpaquePointer?
    private var composingSession: OpaquePointer?
    private var isAutoFlushing: Bool = false
    private var mixedDisplayCursor: Int? = nil   // character-level cursor for mixed content (nil = at end)
    private var savedMixedCursor: Int? = nil     // preserved across candidate selection
    private var lastDisplayCharCount: Int = 0    // character count of last display string
    private var currentClient: IMKTextInput?
    private var spaceCycleMax: Int = 0           // 0=disabled, 1-3=cycle before showing candidates
    private var spaceCycleRemaining: Int = 0     // remaining cycles before showing candidate window
    private var spaceCycleSavedCursor: Int? = nil // chewing cursor position before first cycle
    private var spaceCycleTargets: [String] = [] // pre-computed candidates to cycle through
    private var spaceCycleStep: Int = 0          // current position in targets

    nonisolated(unsafe) private var candidatePanel: CandidatePanel { CandidatePanel.shared }

    /// 選字鍵（從偏好設定載入）
    private var selectionKeys: [Character] = Array("1234567890")

    // MARK: - Lifecycle

    override init!(server: IMKServer!, delegate: Any!, client inputClient: Any!) {
        super.init(server: server, delegate: delegate, client: inputClient)
        initializeEngine()
    }

    deinit {
        if let ctx = chewingContext { chewing_delete(ctx) }
        if let session = composingSession { qb_composing_delete(session) }
    }

    private func initializeEngine() {
        let dictPath = Bundle.main.resourcePath ?? ""
        dbg("CHEWING_PATH = \(dictPath)")
        setenv("CHEWING_PATH", dictPath, 1)
        if kDebugMode {
            setenv("RUST_LOG", "debug", 1)
        }

        chewingContext = chewing_new()
        guard chewingContext != nil else {
            NSLog("QBopomofo: Failed to create chewing context")
            return
        }

        composingSession = qb_composing_new()

        applyPreferences()

        loadCustomDictIfExists()
        setupSIGUSR1Handler()

        dbg("Engine initialized")

        // Listen for preference changes
        NotificationCenter.default.addObserver(self, selector: #selector(preferencesDidChange), name: .qbopomofoPreferencesChanged, object: nil)
    }

    // MARK: - IMKStateSetting

    override func activateServer(_ sender: Any!) {
        currentClient = sender as? IMKTextInput
        if chewingContext == nil { initializeEngine() }
        dbg("Server activated")
    }

    override func deactivateServer(_ sender: Any!) {
        commitComposition(sender)
        currentClient = nil
        dbg("Server deactivated")
    }

    // MARK: - Preferences

    private func applyPreferences() {
        guard let ctx = chewingContext, let session = composingSession else { return }
        let defaults = UserDefaults.standard

        let candPerPage = defaults.integer(forKey: "org.qbopomofo.candPerPage")
        chewing_set_candPerPage(ctx, Int32(candPerPage > 0 ? candPerPage : 9))

        let shiftBehavior = defaults.integer(forKey: "org.qbopomofo.shiftBehavior")
        qb_composing_set_shift_behavior(session, shiftBehavior > 0 ? Int32(shiftBehavior) : 1)

        let selKeysStr = defaults.string(forKey: "org.qbopomofo.selectionKeys") ?? "1234567890"
        selectionKeys = Array(selKeysStr)
        candidatePanel.selectionKeyLabels = selectionKeys.map { String($0) }

        spaceCycleMax = min(max(defaults.integer(forKey: "org.qbopomofo.spaceCycleCount"), -1), 3)
        spaceCycleRemaining = spaceCycleMax
        spaceCycleTargets = []
        spaceCycleStep = 0
        spaceCycleSavedCursor = nil

        // Input mode: 0 = standard (engine 1), 1 = abbreviated (engine 3)
        let inputMode = defaults.integer(forKey: "org.qbopomofo.inputMode")
        let engineValue: Int32 = inputMode == 1 ? 3 : 1
        "chewing.conversion_engine".withCString {
            chewing_config_set_int(ctx, $0, engineValue)
        }

        chewing_set_maxChiSymbolLen(ctx, 20)
        chewing_set_spaceAsSelection(ctx, 1)
        chewing_set_escCleanAllBuf(ctx, 1)
        chewing_set_autoShiftCur(ctx, 1)

        let disableAutoLearn = defaults.bool(forKey: "org.qbopomofo.disableAutoLearn")
        chewing_set_autoLearn(ctx, disableAutoLearn ? 1 : 0)
    }

    @objc private func preferencesDidChange() {
        applyPreferences()
        dbg("Preferences reloaded")
    }

    // MARK: - Custom Dictionary Hot-Reload

    static var customDatPath: String? {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)
            .first?.appendingPathComponent("QBopomofo/custom.dat").path
    }

    // Process-wide singleton: only one DispatchSource per process for SIGUSR1.
    // Fires a Notification so each InputController instance reloads its own context.
    private static let signalSource: DispatchSourceSignal = {
        signal(SIGUSR1, SIG_IGN)
        let src = DispatchSource.makeSignalSource(signal: SIGUSR1, queue: .main)
        src.setEventHandler {
            NotificationCenter.default.post(name: .qbopomofoReloadCustomDict, object: nil)
        }
        src.resume()
        return src
    }()

    private func loadCustomDictIfExists() {
        guard let ctx = chewingContext,
              let path = Self.customDatPath,
              FileManager.default.fileExists(atPath: path) else { return }
        let result = path.withCString { chewing_load_custom_dict(ctx, $0) }
        dbg("Loaded custom dict: \(path) → \(result == 0 ? "OK" : "FAILED")")
    }

    private func setupSIGUSR1Handler() {
        // Trigger lazy init of the process-wide signal source.
        _ = Self.signalSource
        NotificationCenter.default.addObserver(
            self,
            selector: #selector(reloadCustomDict),
            name: .qbopomofoReloadCustomDict,
            object: nil
        )
    }

    @objc private func reloadCustomDict() {
        guard let ctx = chewingContext,
              let path = Self.customDatPath,
              FileManager.default.fileExists(atPath: path) else {
            dbg("Custom dict reload: file not found")
            return
        }
        let result = path.withCString { chewing_load_custom_dict(ctx, $0) }
        dbg("Reloaded custom dict → \(result == 0 ? "OK" : "FAILED")")
    }

    // MARK: - Menu

    override func menu() -> NSMenu! {
        let menu = NSMenu()
        let prefsItem = NSMenuItem(title: "偏好設定…", action: #selector(openPreferences(_:)), keyEquivalent: ",")
        prefsItem.target = self
        menu.addItem(prefsItem)

        menu.addItem(NSMenuItem.separator())

        let reloadItem = NSMenuItem(title: "重新載入自訂詞庫", action: #selector(reloadCustomDict), keyEquivalent: "")
        reloadItem.target = self
        menu.addItem(reloadItem)

        menu.addItem(NSMenuItem.separator())

        let aboutItem = NSMenuItem(title: "關於 Q注音", action: #selector(openAbout(_:)), keyEquivalent: "")
        aboutItem.target = self
        menu.addItem(aboutItem)

        return menu
    }

    @objc func openPreferences(_ sender: Any?) {
        PreferencesWindow.shared.showWindow()
    }

    @objc func openAbout(_ sender: Any?) {
        let alert = NSAlert()
        alert.messageText = "Q注音 QBopomofo"
        alert.informativeText = "版本 0.1.0\nBuild: \(kBuildTimestamp)\n\n基於 libchewing 引擎的注音輸入法"
        alert.alertStyle = .informational
        alert.runModal()
    }

    // MARK: - IMKServerInput

    override func recognizedEvents(_ sender: Any!) -> Int {
        let events: NSEvent.EventTypeMask = [.keyDown, .flagsChanged]
        return Int(events.rawValue)
    }

    override func handle(_ event: NSEvent!, client sender: Any!) -> Bool {
        guard let event = event else { return false }
        guard let ctx = chewingContext, let session = composingSession else {
            dbg("handle called but engine not initialized")
            return false
        }
        guard let client = sender as? IMKTextInput else { return false }

        // Handle Shift key press/release (flagsChanged)
        if event.type == .flagsChanged {
            let isShift = event.modifierFlags.contains(.shift)
            let chineseBuf = getChewingBuffer(ctx)
            let changed = chineseBuf.withCString { cStr in
                qb_composing_handle_shift(session, isShift ? 1 : 0, cStr)
            }
            if changed != 0 {
                // Clear residual bopomofo when switching to English mode
                if qb_composing_is_english(session) != 0 {
                    chewing_clean_bopomofo_buf(ctx)
                }
                updateClientDisplay(ctx: ctx, session: session, client: client)
            }
            return changed != 0
        }

        guard event.type == .keyDown else { return false }

        let keyCode = event.keyCode
        let chars = event.characters ?? ""
        let modifiers = event.modifierFlags
        let shift = modifiers.contains(.shift)

        let isCandMode = inCandidateMode(ctx)
        let capsLock = modifiers.contains(.capsLock)
        dbg("key=\(keyCode) chars=\(chars) shift=\(shift) caps=\(capsLock) candMode=\(isCandMode)")

        // Pass through Command/Control
        if modifiers.contains(.command) || modifiers.contains(.control) { return false }

        // Nothing in buffer/bopomofo and not in candidate mode → pass through navigation keys
        let hasContent = chewing_buffer_Len(ctx) > 0 || chewing_bopomofo_Check(ctx) != 0
            || qb_composing_has_mixed_content(session) != 0
        if let npChar = numpadCharacter(for: keyCode), !isCandMode {
            guard hasContent else { return false }
            return insertASCIIIntoComposition(npChar, ctx: ctx, session: session, client: client, source: "numpad")
        }
        if keyCode == 49 && hasContent && !isCandMode && chewing_bopomofo_Check(ctx) == 0 && spaceCycleMax < 0 {
            spaceCycleRemaining = 0
            spaceCycleTargets = []
            spaceCycleStep = 0
            spaceCycleSavedCursor = nil
            return insertASCIIIntoComposition(" ", ctx: ctx, session: session, client: client, source: "inlineSpace")
        }
        if !hasContent && !isCandMode {
            let passthroughKeys: Set<UInt16> = [
                36, 51, 117, 123, 124, 125, 126, 116, 121, 115, 119, 53, 48
            ]
            if passthroughKeys.contains(keyCode) {
                // Mark Shift as used so release won't toggle mode (e.g. Shift+Enter for newline)
                if shift { qb_composing_mark_shift_used(session) }
                return false
            }
            if keyCode == 49 { // Space → output space
                client.insertText(" ", replacementRange: NSRange(location: NSNotFound, length: 0))
                return true
            }
        }

        // Shift held + typing → English (letters only; punctuation falls through to engine)
        if shift && qb_composing_is_shift_held(session) != 0 {
            if let ch = chars.first, ch.isASCII, ch.isLetter {
                qb_composing_mark_shift_used(session)
                return insertASCIIIntoComposition(ch, ctx: ctx, session: session, client: client, source: "shiftEnglish")
            }
            // Non-letter key while Shift held: mark used so release won't toggle mode
            qb_composing_mark_shift_used(session)
        }

        // English mode
        if qb_composing_is_english(session) != 0 {
            if keyCode == 53 { // Escape
                qb_composing_clear(session)
                chewing_handle_Esc(ctx)
                updateClientDisplay(ctx: ctx, session: session, client: client)
                return true
            }
            if keyCode == 36 { // Enter
                mixedDisplayCursor = nil
                commitAll(ctx: ctx, session: session, client: client, source: "enterEnglish")
                return true
            }
            if keyCode == 53 { // Escape (in English mode, handled above but just in case)
                mixedDisplayCursor = nil
            }
            if keyCode == 51 { // Backspace
                let curPos = mixedDisplayCursor ?? lastDisplayCharCount
                let chinBuf = getChewingBuffer(ctx)
                let bopo = getBopomofoReading(ctx)
                let result = chinBuf.withCString { c in
                    bopo.withCString { b in
                        qb_composing_delete_at_cursor(session, Int32(curPos), c, b)
                    }
                }
                if result == 1 {
                    let newPos = curPos > 0 ? curPos - 1 : 0
                    mixedDisplayCursor = mixedDisplayCursor != nil ? newPos : nil
                    dbg("english delete at cursor \(curPos)")
                    updateClientDisplay(ctx: ctx, session: session, client: client)
                    return true
                }
                // Chinese region or nothing: reset cursor, fall through to chewing
                mixedDisplayCursor = nil
            }
            // Space in English mode
            if keyCode == 49 {
                return insertASCIIIntoComposition(" ", ctx: ctx, session: session, client: client, source: "englishSpace")
            }
            if let ch = chars.first, ch.isASCII, !ch.isNewline {
                return insertASCIIIntoComposition(ch, ctx: ctx, session: session, client: client, source: "englishChar")
            }
        }

        // Backspace with mixed content — cursor-aware delete (skip if in candidate mode)
        if keyCode == 51 && qb_composing_has_mixed_content(session) != 0 && !isCandMode {
            let curPos = mixedDisplayCursor ?? lastDisplayCharCount
            let chinBuf = getChewingBuffer(ctx)
            let bopo = getBopomofoReading(ctx)
            let result = chinBuf.withCString { c in
                bopo.withCString { b in
                    qb_composing_delete_at_cursor(session, Int32(curPos), c, b)
                }
            }
            if result == 1 {
                // English char deleted
                mixedDisplayCursor = curPos > 0 ? curPos - 1 : 0
                dbg("mixed delete english at cursor \(curPos) → \(mixedDisplayCursor!)")
                updateClientDisplay(ctx: ctx, session: session, client: client)
                return true
            }
            if result == 2 {
                // Chinese region — sync chewing cursor and let engine handle backspace
                let chewCur = chinBuf.withCString { c in
                    bopo.withCString { b in
                        qb_composing_display_to_chewing_cursor(session, Int32(curPos), c, b)
                    }
                }
                if chewCur >= 0 {
                    syncChewingCursor(ctx: ctx, target: Int(chewCur))
                }
                chewing_handle_Backspace(ctx)
                mixedDisplayCursor = curPos > 0 ? curPos - 1 : 0
                dbg("mixed delete chinese at cursor \(curPos) → chewing=\(chewCur)")
                updateClientDisplay(ctx: ctx, session: session, client: client)
                return true
            }
        }

        // Enter/Escape with mixed content
        if keyCode == 36 && qb_composing_has_mixed_content(session) != 0 && !isCandMode {
            mixedDisplayCursor = nil
            commitAll(ctx: ctx, session: session, client: client, source: "enterMixed")
            return true
        }
        if keyCode == 53 && qb_composing_has_mixed_content(session) != 0 && !isCandMode {
            mixedDisplayCursor = nil
            qb_composing_clear(session)
            chewing_handle_Esc(ctx)
            updateClientDisplay(ctx: ctx, session: session, client: client)
            return true
        }

        // Candidate mode — custom CandidatePanel handles all navigation
        if isCandMode {
            switch keyCode {
            case 125: // Down — next candidate, or next page if at bottom
                if !candidatePanel.highlightNext() {
                    chewing_handle_Right(ctx)
                    updateClientDisplay(ctx: ctx, session: session, client: client)
                    dbg("cand ↓ → next page (page \(chewing_cand_CurrentPage(ctx)+1)/\(chewing_cand_TotalPage(ctx)))")
                } else {
                    dbg("cand ↓ → idx=\(candidatePanel.highlightedIndex)")
                }
                return true
            case 126: // Up — previous candidate
                candidatePanel.highlightPrevious()
                dbg("cand ↑ → idx=\(candidatePanel.highlightedIndex)")
                return true
            case 36: // Enter — select current candidate
                selectCandidateAndLog(ctx: ctx, session: session, client: client, index: candidatePanel.highlightedIndex, source: "enter")
                return true
            case 124: // Right — next page
                chewing_handle_Right(ctx)
                dbg("cand page → (page \(chewing_cand_CurrentPage(ctx)+1)/\(chewing_cand_TotalPage(ctx)))")
                updateClientDisplay(ctx: ctx, session: session, client: client)
                return true
            case 123: // Left — previous page
                chewing_handle_Left(ctx)
                dbg("cand page ← (page \(chewing_cand_CurrentPage(ctx)+1)/\(chewing_cand_TotalPage(ctx)))")
                updateClientDisplay(ctx: ctx, session: session, client: client)
                return true
            case 49: // Space — next page
                chewing_handle_Right(ctx)
                dbg("cand space page → (page \(chewing_cand_CurrentPage(ctx)+1)/\(chewing_cand_TotalPage(ctx)))")
                updateClientDisplay(ctx: ctx, session: session, client: client)
                return true
            case 51: // Backspace — close candidates and delete
                chewing_cand_close(ctx)
                candidatePanel.hidePanel()
                chewing_handle_Backspace(ctx)
                dbg("cand backspace → close and delete")
                restoreMixedCursorIfNeeded()
                updateClientDisplay(ctx: ctx, session: session, client: client)
                return true
            case 53: // Escape — close candidates
                chewing_cand_close(ctx)
                candidatePanel.hidePanel()
                dbg("cand cancel")
                restoreMixedCursorIfNeeded()
                updateClientDisplay(ctx: ctx, session: session, client: client)
                return true
            default:
                // Selection keys (configurable: 1234567890 or asdfghjkl;)
                if let ch = chars.first,
                   let idx = selectionKeys.firstIndex(of: ch),
                   idx < candidatePanel.candidates.count {
                    selectCandidateAndLog(ctx: ctx, session: session, client: client, index: idx, source: "key:\(ch)")
                    return true
                }
            }
        }

        // Mixed content cursor navigation — Left/Right move through entire display (skip if in candidate mode)
        if qb_composing_has_mixed_content(session) != 0 && !isCandMode && (keyCode == 123 || keyCode == 124) {
            if keyCode == 123 { // Left
                let pos = mixedDisplayCursor ?? lastDisplayCharCount
                if pos > 0 { mixedDisplayCursor = pos - 1 }
                dbg("mixed cursor ← → pos=\(mixedDisplayCursor ?? -1)")
            } else { // Right
                let pos = mixedDisplayCursor ?? lastDisplayCharCount
                if pos < lastDisplayCharCount { mixedDisplayCursor = pos + 1 } else { mixedDisplayCursor = nil }
                dbg("mixed cursor → → pos=\(mixedDisplayCursor ?? -1)")
            }
            updateClientDisplay(ctx: ctx, session: session, client: client)
            return true
        }

        // Mixed content: sync chewing engine cursor before delegating to engine
        if qb_composing_has_mixed_content(session) != 0, let curPos = mixedDisplayCursor {
            let chinBuf = getChewingBuffer(ctx)
            let bopo = getBopomofoReading(ctx)
            let chewCur = chinBuf.withCString { c in
                bopo.withCString { b in
                    qb_composing_display_to_chewing_cursor(session, Int32(curPos), c, b)
                }
            }
            if chewCur >= 0 {
                syncChewingCursor(ctx: ctx, target: Int(chewCur))
                dbg("mixed→chewing cursor sync: display=\(curPos) → chewing=\(chewCur)")
            }
            // Save cursor for restore after candidate selection
            savedMixedCursor = curPos
        }
        mixedDisplayCursor = nil

        // Space cycle: silently replace with next candidate before opening the window
        if keyCode == 49 && !isCandMode && spaceCycleRemaining > 0 && chewing_buffer_Len(ctx) > 0 && chewing_bopomofo_Check(ctx) == 0 {
            // First cycle: enter cand mode, compute all targets upfront
            if spaceCycleTargets.isEmpty {
                spaceCycleSavedCursor = Int(chewing_cursor_Current(ctx))

                // Get the original text at cursor
                // When cursor is at end of buffer, engine uses cursor-1 (same as symbol_for_select)
                let buf = Array(getChewingBuffer(ctx))
                let cur = spaceCycleSavedCursor ?? 0
                let selectPos = cur >= buf.count ? max(cur - 1, 0) : cur

                chewing_handle_Space(ctx) // enters selecting mode

                if inCandidateMode(ctx) {
                    let candidates = getCandidateList(ctx)

                    // Exclude any candidate that matches the current buffer text at cursor
                    // (candidates may have mixed lengths: 1-char and 2-char phrases)
                    var excluded = Set<String>()
                    for cand in candidates {
                        let candLen = cand.count
                        let end = min(selectPos + candLen, buf.count)
                        if selectPos < end && String(buf[selectPos..<end]) == cand {
                            excluded.insert(cand)
                        }
                    }

                    // Pre-compute distinct candidates to cycle through
                    var seen = excluded
                    for cand in candidates {
                        if !seen.contains(cand) {
                            spaceCycleTargets.append(cand)
                            seen.insert(cand)
                            if spaceCycleTargets.count >= spaceCycleMax { break }
                        }
                    }
                    dbg("spaceCycle: excluded=\(excluded) targets=\(spaceCycleTargets)")

                    if spaceCycleTargets.isEmpty {
                        // No different candidates at all — stay in cand mode and show panel
                        spaceCycleRemaining = 0
                    } else {
                        // Select the first target
                        let target = spaceCycleTargets[0]
                        if let idx = candidates.firstIndex(of: target) {
                            chewing_cand_choose_by_index(ctx, Int32(idx))
                            dbg("spaceCycle: → '\(target)' step=0")
                        }
                        spaceCycleStep = 1
                        spaceCycleRemaining -= 1
                        syncChewingCursor(ctx: ctx, target: spaceCycleSavedCursor ?? 0)

                        if qb_composing_has_mixed_content(session) != 0 {
                            let newBuf = getChewingBuffer(ctx)
                            newBuf.withCString { c in qb_composing_resync_chinese(session, c) }
                        }
                    }
                } else {
                    spaceCycleRemaining = 0
                    dbg("spaceCycle: cand mode not entered, aborting")
                }
            } else if spaceCycleStep < spaceCycleTargets.count {
                // Subsequent cycles: restore cursor, re-enter cand mode, select next target
                syncChewingCursor(ctx: ctx, target: spaceCycleSavedCursor ?? 0)
                chewing_handle_Space(ctx)

                if inCandidateMode(ctx) {
                    let candidates = getCandidateList(ctx)
                    let target = spaceCycleTargets[spaceCycleStep]
                    if let idx = candidates.firstIndex(of: target) {
                        chewing_cand_choose_by_index(ctx, Int32(idx))
                        dbg("spaceCycle: → '\(target)' step=\(spaceCycleStep)")
                    } else {
                        // Target not found in current list — abort, show panel
                        spaceCycleRemaining = 0
                        dbg("spaceCycle: target '\(target)' not found, opening panel")
                    }
                    spaceCycleStep += 1
                    spaceCycleRemaining -= 1
                    syncChewingCursor(ctx: ctx, target: spaceCycleSavedCursor ?? 0)

                    if qb_composing_has_mixed_content(session) != 0 {
                        let newBuf = getChewingBuffer(ctx)
                        newBuf.withCString { c in qb_composing_resync_chinese(session, c) }
                    }
                } else {
                    spaceCycleRemaining = 0
                }
            } else {
                spaceCycleRemaining = 0
            }

            restoreMixedCursorIfNeeded()
            updateClientDisplay(ctx: ctx, session: session, client: client)
            return true
        }

        // Chinese mode — send to chewing engine
        let handled = processChewingKey(ctx: ctx, keyCode: keyCode, chars: chars)
        if handled { updateClientDisplay(ctx: ctx, session: session, client: client) }
        return handled
    }

    // MARK: - Key Processing (Chinese only)

    private func processChewingKey(ctx: OpaquePointer, keyCode: UInt16, chars: String) -> Bool {
        // Reset space cycle state on non-Space keys
        if keyCode != 49 {
            spaceCycleRemaining = spaceCycleMax
            spaceCycleTargets = []
            spaceCycleStep = 0
            spaceCycleSavedCursor = nil
        }

        switch keyCode {
        case 36: chewing_handle_Enter(ctx); return true
        case 51: chewing_handle_Backspace(ctx); return true
        case 53: chewing_handle_Esc(ctx); return true
        case 49: chewing_handle_Space(ctx); return true
        case 48: chewing_handle_Tab(ctx); return true
        case 117: chewing_handle_Del(ctx); return true
        case 123: chewing_handle_Left(ctx); return true
        case 124: chewing_handle_Right(ctx); return true
        case 125: // Down — open candidate window if buffer exists
            if chewing_cand_TotalPage(ctx) > 0 {
                chewing_handle_Down(ctx)
            } else {
                chewing_cand_open(ctx)
            }
            return true
        case 126: chewing_handle_Up(ctx); return true
        case 116: chewing_handle_PageUp(ctx); return true
        case 121: chewing_handle_PageDown(ctx); return true
        case 115: chewing_handle_Home(ctx); return true
        case 119: chewing_handle_End(ctx); return true
        default: break
        }
        guard let firstChar = chars.first, let ascii = firstChar.asciiValue else { return false }
        chewing_handle_Default(ctx, Int32(ascii))
        return true
    }

    // MARK: - Display Update

    private func updateClientDisplay(ctx: OpaquePointer, session: OpaquePointer, client: IMKTextInput) {
        // Commit text from chewing engine
        if chewing_commit_Check(ctx) != 0 {
            if let commitStr = chewing_commit_String(ctx) {
                let text = String(cString: commitStr)
                chewing_free(commitStr)
                _ = chewing_ack(ctx)

                if qb_composing_has_mixed_content(session) != 0 {
                    // Mixed content: commit through composing session to include English parts
                    let result = text.withCString { cStr -> String in
                        if let ptr = qb_composing_commit_all(session, cStr) {
                            let s = String(cString: ptr)
                            chewing_free(ptr)
                            return s
                        }
                        return text
                    }
                    dbg("commit='\(result)' [source:updateDisplayMixed]")
                    client.insertText(result, replacementRange: NSRange(location: NSNotFound, length: 0))
                    client.setMarkedText("", selectionRange: NSRange(location: 0, length: 0), replacementRange: NSRange(location: NSNotFound, length: 0))
                    mixedDisplayCursor = nil
                    savedMixedCursor = nil
                    return
                }

                dbg("commit='\(text)' [source:updateDisplay]")
                client.insertText(text, replacementRange: NSRange(location: NSNotFound, length: 0))
            } else {
                _ = chewing_ack(ctx)
            }
        }

        // Check candidate state
        let candTotal = chewing_cand_TotalPage(ctx)
        let inCandMode = chewing_cand_CheckDone(ctx) == 0 && candTotal > 0

        // Build display via Rust session (handles mixed Chinese/English)
        let chinese = getChewingBuffer(ctx)
        let bopomofo = getBopomofoReading(ctx)
        let hasMixed = qb_composing_has_mixed_content(session) != 0

        // Resync Chinese snapshots after engine may have re-segmented the buffer
        // (e.g. 是→事變). Without this, stale snapshots cause duplicated output.
        if hasMixed {
            chinese.withCString { c in qb_composing_resync_chinese(session, c) }
        }
        let chewingCursor = Int(chewing_cursor_Current(ctx))
        dbg("display chinese='\(chinese)' bopo='\(bopomofo)' bufLen=\(chewing_buffer_Len(ctx)) cursor=\(chewingCursor) candPages=\(candTotal) candMode=\(inCandMode)")

        let display: String
        if !hasMixed {
            // Pure Chinese: insert bopomofo at cursor position, not at end
            let clampedCursor = min(chewingCursor, chinese.count)
            let charIndex = chinese.index(chinese.startIndex, offsetBy: clampedCursor)
            display = String(chinese[chinese.startIndex..<charIndex]) + bopomofo + String(chinese[charIndex...])
        } else {
            display = chinese.withCString { chinBuf in
                bopomofo.withCString { bopoBuf -> String in
                    if let ptr = qb_composing_build_display(session, chinBuf, bopoBuf) {
                        let s = String(cString: ptr)
                        chewing_free(ptr)
                        return s
                    }
                    return chinese + bopomofo
                }
            }
        }

        // Auto-flush for mixed content only (pure Chinese overflow is handled by the engine's maxChiSymbolLen).
        // Only flush when no bopomofo is in progress to avoid clearing marked text mid-input,
        // which causes garbled output in terminal emulators (iTerm2, CLI apps).
        if !isAutoFlushing && display.count > 20
            && qb_composing_has_mixed_content(session) != 0
            && chewing_bopomofo_Check(ctx) == 0 {
            isAutoFlushing = true
            commitAll(ctx: ctx, session: session, client: client, source: "autoFlush")
            isAutoFlushing = false
            return
        }

        if !display.isEmpty {
            let nsDisplay = display as NSString
            lastDisplayCharCount = display.count

            // Compute cursor position (UTF-16 offset)
            let cursorPos: Int
            if !hasMixed {
                // Pure Chinese mode: display = chinese[0..cursor] + bopomofo + chinese[cursor..]
                // Cursor goes after the bopomofo insertion
                let clampedCursor = min(chewingCursor, chinese.count)
                let charIndex = chinese.index(chinese.startIndex, offsetBy: clampedCursor)
                let beforeCursorUTF16 = chinese[chinese.startIndex..<charIndex].utf16.count
                let bopoUTF16 = (bopomofo as NSString).length
                cursorPos = beforeCursorUTF16 + bopoUTF16
            } else if let mcp = mixedDisplayCursor {
                // Mixed content with explicit cursor position
                let clampedPos = min(mcp, display.count)
                let idx = display.index(display.startIndex, offsetBy: clampedPos)
                cursorPos = display[display.startIndex..<idx].utf16.count
            } else {
                // Mixed content: cursor at end
                cursorPos = nsDisplay.length
            }

            // Build attributed string: thick underline on char at cursor, thin on rest
            let attrStr = NSMutableAttributedString(string: display)
            let fullRange = NSRange(location: 0, length: nsDisplay.length)
            attrStr.addAttribute(.underlineStyle, value: NSUnderlineStyle.single.rawValue, range: fullRange)
            attrStr.addAttribute(.markedClauseSegment, value: 0, range: fullRange)
            if cursorPos < nsDisplay.length {
                let cursorCharRange = nsDisplay.rangeOfComposedCharacterSequence(at: cursorPos)
                attrStr.addAttribute(.underlineStyle, value: NSUnderlineStyle.thick.rawValue, range: cursorCharRange)
                attrStr.addAttribute(.markedClauseSegment, value: 1, range: cursorCharRange)
            }

            client.setMarkedText(
                attrStr,
                selectionRange: NSRange(location: cursorPos, length: 0),
                replacementRange: NSRange(location: NSNotFound, length: 0)
            )
        } else {
            client.setMarkedText(
                "",
                selectionRange: NSRange(location: 0, length: 0),
                replacementRange: NSRange(location: NSNotFound, length: 0)
            )
        }

        // Show/hide candidate panel
        if inCandMode {
            let candList = getCandidateList(ctx)
            let page = Int(chewing_cand_CurrentPage(ctx))
            let totalPages = Int(chewing_cand_TotalPage(ctx))
            // Only show candidates for current page
            let perPage = Int(chewing_get_candPerPage(ctx))
            let pageList = Array(candList.prefix(perPage))
            candidatePanel.setCandidates(pageList, page: page, totalPages: totalPages)
            dbg("candidatePanel count=\(pageList.count) page=\(page+1)/\(totalPages)")

            // Position below cursor
            let cursorPoint = getCursorPosition(client: client)
            candidatePanel.show(at: cursorPoint)
        } else {
            if candidatePanel.isPanelVisible { candidatePanel.hidePanel() }
        }
    }

    // MARK: - Commit

    private func commitAll(ctx: OpaquePointer, session: OpaquePointer, client: IMKTextInput, source: String) {
        // Hide candidate panel
        candidatePanel.hidePanel()

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

        let result = finalChinese.withCString { cStr -> String in
            if let ptr = qb_composing_commit_all(session, cStr) {
                let s = String(cString: ptr)
                chewing_free(ptr)
                return s
            }
            return finalChinese
        }

        if !result.isEmpty {
            dbg("commitAll='\(result)' [source:\(source)]")
            client.insertText(result, replacementRange: NSRange(location: NSNotFound, length: 0))
        }
        client.setMarkedText("", selectionRange: NSRange(location: 0, length: 0), replacementRange: NSRange(location: NSNotFound, length: 0))
        chewing_Reset(ctx)
        mixedDisplayCursor = nil
        savedMixedCursor = nil
        spaceCycleRemaining = spaceCycleMax
        spaceCycleTargets = []
        spaceCycleStep = 0
        spaceCycleSavedCursor = nil
    }

    override func commitComposition(_ sender: Any!) {
        guard let ctx = chewingContext, let session = composingSession else { return }
        guard let client = sender as? IMKTextInput else {
            dbg("commitComposition called (no client)")
            return
        }
        // Skip if nothing to commit
        if chewing_buffer_Len(ctx) == 0 && chewing_bopomofo_Check(ctx) == 0 {
            dbg("commitComposition called (empty, skip)")
            return
        }
        dbg("commitComposition called")
        commitAll(ctx: ctx, session: session, client: client, source: "commitComposition")
    }


    // MARK: - Helpers

    /// Move the chewing engine cursor to the target position by sending Left/Right keys.
    private func syncChewingCursor(ctx: OpaquePointer, target: Int) {
        let current = Int(chewing_cursor_Current(ctx))
        if target < current {
            for _ in 0..<(current - target) { chewing_handle_Left(ctx) }
        } else if target > current {
            for _ in 0..<(target - current) { chewing_handle_Right(ctx) }
        }
    }

    private func numpadCharacter(for keyCode: UInt16) -> Character? {
        switch keyCode {
        case 82: return "0"
        case 83: return "1"
        case 84: return "2"
        case 85: return "3"
        case 86: return "4"
        case 87: return "5"
        case 88: return "6"
        case 89: return "7"
        case 91: return "8"
        case 92: return "9"
        case 75: return "/"
        case 67: return "*"
        case 78: return "-"
        case 69: return "+"
        case 65: return "."
        case 81: return "="
        default: return nil
        }
    }

    private func preferredInsertCursor(ctx: OpaquePointer, session: OpaquePointer) -> Int? {
        if let cursor = mixedDisplayCursor {
            return cursor
        }
        if qb_composing_has_mixed_content(session) != 0 {
            return lastDisplayCharCount
        }
        // Only return a cursor position if there's actual content in the buffer
        if chewing_buffer_Len(ctx) > 0 && chewing_bopomofo_Check(ctx) == 0 {
            return Int(chewing_cursor_Current(ctx))
        }
        return nil
    }

    private func insertASCIIIntoComposition(
        _ ch: Character,
        ctx: OpaquePointer,
        session: OpaquePointer,
        client: IMKTextInput,
        source: String
    ) -> Bool {
        guard ch.isASCII, let ascii = ch.asciiValue else { return false }

        let chinBuf = getChewingBuffer(ctx)
        let bopo = getBopomofoReading(ctx)

        if let curPos = preferredInsertCursor(ctx: ctx, session: session) {
            let handled = chinBuf.withCString { c in
                bopo.withCString { b in
                    qb_composing_insert_at_cursor(session, ascii, Int32(curPos), c, b)
                }
            }
            if handled != 0 {
                mixedDisplayCursor = curPos + 1
                dbg("\(source) insert '\(ch)' at cursor \(curPos) → \(mixedDisplayCursor!)")
                updateClientDisplay(ctx: ctx, session: session, client: client)
                return true
            }
            if mixedDisplayCursor != nil {
                mixedDisplayCursor = nil
            }
        }

        let directCommit = chinBuf.withCString { cStr in
            qb_composing_type_english(session, ascii, cStr)
        }
        if directCommit != 0 {
            dbg("insertText='\(ch)' [source:\(source)]")
            client.insertText(String(ch), replacementRange: NSRange(location: NSNotFound, length: 0))
        } else {
            updateClientDisplay(ctx: ctx, session: session, client: client)
        }
        return true
    }

    private func getChewingBuffer(_ ctx: OpaquePointer) -> String {
        if chewing_buffer_Len(ctx) > 0, let bufStr = chewing_buffer_String(ctx) {
            let s = String(cString: bufStr)
            chewing_free(bufStr)
            return s
        }
        return ""
    }

    private func inCandidateMode(_ ctx: OpaquePointer) -> Bool {
        chewing_cand_CheckDone(ctx) == 0 && chewing_cand_TotalPage(ctx) > 0
    }

    private func getCandidateList(_ ctx: OpaquePointer) -> [String] {
        var list: [String] = []
        chewing_cand_Enumerate(ctx)
        while chewing_cand_hasNext(ctx) != 0 {
            if let s = chewing_cand_String(ctx) {
                list.append(String(cString: s))
                chewing_free(s)
            }
        }
        return list
    }

    private func getCursorPosition(client: IMKTextInput) -> NSPoint {
        var lineRect = NSRect.zero
        client.attributes(forCharacterIndex: 0, lineHeightRectangle: &lineRect)
        // Return bottom-left of the line rect (candidate panel appears below)
        return NSPoint(x: lineRect.origin.x, y: lineRect.origin.y)
    }

    /// Select a candidate and log the correction if the buffer changed
    private func selectCandidateAndLog(ctx: OpaquePointer, session: OpaquePointer, client: IMKTextInput, index: Int, source: String) {
        let bufferBefore = getChewingBuffer(ctx)
        chewing_cand_choose_by_index(ctx, Int32(index))
        let bufferAfter = getChewingBuffer(ctx)

        dbg("cand \(source) select idx=\(index)")

        // Resync Chinese snapshots in composing session after candidate selection
        if qb_composing_has_mixed_content(session) != 0 {
            let newBuf = bufferAfter
            newBuf.withCString { c in
                qb_composing_resync_chinese(session, c)
            }
        }

        if kDebugMode && bufferBefore != bufferAfter {
            // Extract context: 3 chars around the change
            let before = Array(bufferBefore)
            let after = Array(bufferAfter)
            // Find first differing position
            var diffPos = 0
            while diffPos < min(before.count, after.count) && before[diffPos] == after[diffPos] {
                diffPos += 1
            }
            // Find last differing position from end
            var diffEnd = 0
            while diffEnd < min(before.count, after.count) - diffPos
                    && before[before.count - 1 - diffEnd] == after[after.count - 1 - diffEnd] {
                diffEnd += 1
            }
            let ctxStart = max(0, diffPos - 3)
            let ctxEnd = min(before.count, before.count - diffEnd + 3)
            let contextBefore = String(before[ctxStart..<ctxEnd])
            let ctxEndAfter = min(after.count, after.count - diffEnd + 3)
            let contextAfter = String(after[ctxStart..<ctxEndAfter])
            let original = String(before[diffPos..<(before.count - diffEnd)])
            let corrected = String(after[diffPos..<(after.count - diffEnd)])
            logCorrection("'\(original)'→'\(corrected)' context: '\(contextBefore)'→'\(contextAfter)' full: '\(bufferBefore)'→'\(bufferAfter)'")
        }

        restoreMixedCursorIfNeeded()
        updateClientDisplay(ctx: ctx, session: session, client: client)
    }

    /// Restore mixedDisplayCursor after exiting candidate mode in mixed content.
    private func restoreMixedCursorIfNeeded() {
        if let saved = savedMixedCursor {
            mixedDisplayCursor = saved
            savedMixedCursor = nil
        }
    }

    private func getBopomofoReading(_ ctx: OpaquePointer) -> String {
        if chewing_bopomofo_Check(ctx) != 0, let bopoStr = chewing_bopomofo_String(ctx) {
            let s = String(cString: bopoStr)
            chewing_free(bopoStr)
            return s
        }
        return ""
    }
}
