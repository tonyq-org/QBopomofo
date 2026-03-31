import Cocoa
@preconcurrency import InputMethodKit
import CChewing

/// Debug mode: writes to /tmp/qbopomofo.log when QBOPOMOFO_DEBUG env is set
private let kDebugMode = ProcessInfo.processInfo.environment["QBOPOMOFO_DEBUG"] != nil
nonisolated(unsafe) private var debugLogHandle: FileHandle? = {
    guard kDebugMode else { return nil }
    let path = "/tmp/qbopomofo.log"
    FileManager.default.createFile(atPath: path, contents: nil)
    return FileHandle(forWritingAtPath: path)
}()

private func dbg(_ msg: String) {
    guard kDebugMode else { return }
    let ts = DateFormatter.localizedString(from: Date(), dateStyle: .none, timeStyle: .medium)
    let line = "[\(ts)] \(msg)\n"
    if let data = line.data(using: .utf8) {
        debugLogHandle?.seekToEndOfFile()
        debugLogHandle?.write(data)
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
    private var currentClient: IMKTextInput?
    nonisolated(unsafe) private static var candidateWindow: IMKCandidates?

    // MARK: - Lifecycle

    override init!(server: IMKServer!, delegate: Any!, client inputClient: Any!) {
        super.init(server: server, delegate: delegate, client: inputClient)
        if QBopomofoInputController.candidateWindow == nil {
            // IMKCandidates init requires IMKServer; safe because we're always on main thread
            nonisolated(unsafe) let srv = server!
            if let candWin = IMKCandidates(server: srv, panelType: kIMKSingleColumnScrollingCandidatePanel) {
                candWin.setAttributes([IMKCandidatesSendServerKeyEventFirst: NSNumber(value: true)])
                QBopomofoInputController.candidateWindow = candWin
            }
        }
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

        chewingContext = chewing_new()
        guard chewingContext != nil else {
            NSLog("QBopomofo: Failed to create chewing context")
            return
        }

        composingSession = qb_composing_new()

        applyPreferences()

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

        chewing_set_maxChiSymbolLen(ctx, 20)
        chewing_set_spaceAsSelection(ctx, 1)
        chewing_set_escCleanAllBuf(ctx, 1)
        chewing_set_autoShiftCur(ctx, 1)
    }

    @objc private func preferencesDidChange() {
        applyPreferences()
        dbg("Preferences reloaded")
    }

    // MARK: - Menu

    override func menu() -> NSMenu! {
        let menu = NSMenu()
        let prefsItem = NSMenuItem(title: "偏好設定…", action: #selector(openPreferences(_:)), keyEquivalent: ",")
        prefsItem.target = self
        menu.addItem(prefsItem)

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
            if changed != 0 { updateClientDisplay(ctx: ctx, session: session, client: client) }
            return changed != 0
        }

        guard event.type == .keyDown else { return false }

        let keyCode = event.keyCode
        let chars = event.characters ?? ""
        let modifiers = event.modifierFlags
        let shift = modifiers.contains(.shift)

        dbg("key=\(keyCode) chars=\(chars)")

        // Pass through Command/Control and numpad keys
        if modifiers.contains(.command) || modifiers.contains(.control) { return false }
        // Numpad digit keys (keyCodes 82-92) → pass through directly (not as bopomofo)
        let numpadDigits: Set<UInt16> = [82,83,84,85,86,87,88,89,91,92] // 0-9 on numpad
        if numpadDigits.contains(keyCode) { return false }

        // Shift held + typing → English (letters only; punctuation falls through to engine)
        if shift && qb_composing_is_shift_held(session) != 0 {
            if let ch = chars.first, ch.isASCII, ch.isLetter {
                let chinBuf = getChewingBuffer(ctx)
                let directCommit = chinBuf.withCString { cStr in
                    qb_composing_type_english(session, UInt8(ch.asciiValue ?? 0), cStr)
                }
                if directCommit != 0 {
                    dbg("insertText='\(ch)' [source:shiftEnglish]")
                    client.insertText(String(ch), replacementRange: NSRange(location: NSNotFound, length: 0))
                } else {
                    updateClientDisplay(ctx: ctx, session: session, client: client)
                }
                return true
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
                commitAll(ctx: ctx, session: session, client: client, source: "enterEnglish")
                return true
            }
            if keyCode == 51 { // Backspace
                if qb_composing_backspace_english(session) != 0 {
                    updateClientDisplay(ctx: ctx, session: session, client: client)
                    return true
                }
            }
            // Space in English mode
            if keyCode == 49 {
                let chinBuf = getChewingBuffer(ctx)
                let directCommit = chinBuf.withCString { cStr in
                    qb_composing_type_english(session, UInt8(Character(" ").asciiValue!), cStr)
                }
                if directCommit != 0 {
                    dbg("insertText=' ' [source:englishSpace]")
                    client.insertText(" ", replacementRange: NSRange(location: NSNotFound, length: 0))
                } else {
                    updateClientDisplay(ctx: ctx, session: session, client: client)
                }
                return true
            }
            if let ch = chars.first, ch.isASCII, !ch.isNewline {
                let chinBuf = getChewingBuffer(ctx)
                let directCommit = chinBuf.withCString { cStr in
                    qb_composing_type_english(session, UInt8(ch.asciiValue ?? 0), cStr)
                }
                if directCommit != 0 {
                    dbg("insertText='\(ch)' [source:englishChar]")
                    client.insertText(String(ch), replacementRange: NSRange(location: NSNotFound, length: 0))
                } else {
                    updateClientDisplay(ctx: ctx, session: session, client: client)
                }
                return true
            }
        }

        // Backspace with mixed content — try deleting from session first
        if keyCode == 51 && qb_composing_has_mixed_content(session) != 0 {
            if qb_composing_backspace_english(session) != 0 {
                updateClientDisplay(ctx: ctx, session: session, client: client)
                return true
            }
        }

        // Enter/Escape with mixed content
        if keyCode == 36 && qb_composing_has_mixed_content(session) != 0 {
            commitAll(ctx: ctx, session: session, client: client, source: "enterMixed")
            return true
        }
        if keyCode == 53 && qb_composing_has_mixed_content(session) != 0 {
            qb_composing_clear(session)
            chewing_handle_Esc(ctx)
            updateClientDisplay(ctx: ctx, session: session, client: client)
            return true
        }

        // Candidate mode — let IMKCandidates handle Up/Down/Enter navigation,
        // we only intercept Left/Right (paging), Esc, and number keys.
        if inCandidateMode(ctx) {
            switch keyCode {
            case 125, 126: // Down/Up — let IMKCandidates handle highlight navigation
                dbg("cand \(keyCode == 125 ? "↓" : "↑") → pass to IMKCandidates")
                return false
            case 36: // Enter — let IMKCandidates handle selection (calls candidateSelected)
                dbg("cand enter → pass to IMKCandidates")
                return false
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
            case 49: // Space — let IMKCandidates handle (next candidate or confirm)
                dbg("cand space → pass to IMKCandidates")
                return false
            case 53: // Escape — close candidates
                chewing_cand_close(ctx)
                dbg("cand cancel")
                updateClientDisplay(ctx: ctx, session: session, client: client)
                return true
            default:
                // Number keys 1-9 select directly
                if let ch = chars.first, ch >= "1" && ch <= "9" {
                    let idx = Int(ch.asciiValue! - Character("1").asciiValue!)
                    chewing_cand_choose_by_index(ctx, Int32(idx))
                    dbg("cand select #\(idx + 1)")
                    updateClientDisplay(ctx: ctx, session: session, client: client)
                    return true
                }
            }
        }

        // Chinese mode: space with no buffer → output space directly
        if keyCode == 49 && chewing_buffer_Len(ctx) == 0 && chewing_bopomofo_Check(ctx) == 0 {
            client.insertText(" ", replacementRange: NSRange(location: NSNotFound, length: 0))
            return true
        }

        // Chinese mode — send to chewing engine
        let handled = processChewingKey(ctx: ctx, keyCode: keyCode, chars: chars)
        if handled { updateClientDisplay(ctx: ctx, session: session, client: client) }
        return handled
    }

    // MARK: - Key Processing (Chinese only)

    private func processChewingKey(ctx: OpaquePointer, keyCode: UInt16, chars: String) -> Bool {
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
                dbg("commit='\(text)' [source:updateDisplay]")
                client.insertText(text, replacementRange: NSRange(location: NSNotFound, length: 0))
                chewing_free(commitStr)
            }
            _ = chewing_ack(ctx)
        }

        // Check candidate state
        let candTotal = chewing_cand_TotalPage(ctx)
        let inCandMode = chewing_cand_CheckDone(ctx) == 0 && candTotal > 0

        // Build display via Rust session (handles mixed Chinese/English)
        let chinese = getChewingBuffer(ctx)
        let bopomofo = getBopomofoReading(ctx)
        dbg("display chinese='\(chinese)' bopo='\(bopomofo)' bufLen=\(chewing_buffer_Len(ctx)) candPages=\(candTotal) candMode=\(inCandMode)")
        let display = chinese.withCString { chinBuf in
            bopomofo.withCString { bopoBuf -> String in
                if let ptr = qb_composing_build_display(session, chinBuf, bopoBuf) {
                    let s = String(cString: ptr)
                    chewing_free(ptr)
                    return s
                }
                return chinese + bopomofo
            }
        }

        // Auto-flush: composing display > 20 chars → commit all
        if !isAutoFlushing && display.count > 20 {
            isAutoFlushing = true
            commitAll(ctx: ctx, session: session, client: client, source: "autoFlush")
            isAutoFlushing = false
            return
        }

        if !display.isEmpty {
            client.setMarkedText(
                display,
                selectionRange: NSRange(location: display.count, length: 0),
                replacementRange: NSRange(location: NSNotFound, length: 0)
            )
        } else {
            client.setMarkedText(
                "",
                selectionRange: NSRange(location: 0, length: 0),
                replacementRange: NSRange(location: NSNotFound, length: 0)
            )
        }

        // Show/hide candidate window
        if let candWin = QBopomofoInputController.candidateWindow {
            if inCandMode {
                candWin.update()
                candWin.show(kIMKLocateCandidatesBelowHint)
            } else {
                if candWin.isVisible() { candWin.hide() }
            }
        }
    }

    // MARK: - Commit

    private func commitAll(ctx: OpaquePointer, session: OpaquePointer, client: IMKTextInput, source: String) {
        // Hide candidate window
        QBopomofoInputController.candidateWindow?.hide()

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
    }

    override func commitComposition(_ sender: Any!) {
        guard let ctx = chewingContext, let session = composingSession else { return }
        guard let client = sender as? IMKTextInput else { return }
        dbg("commitComposition called")
        commitAll(ctx: ctx, session: session, client: client, source: "commitComposition")
        chewing_Reset(ctx)
    }

    // MARK: - Candidates

    override func candidates(_ sender: Any!) -> [Any]! {
        guard let ctx = chewingContext else { return nil }
        guard chewing_cand_TotalPage(ctx) > 0 else { return nil }
        var candidates: [String] = []
        chewing_cand_Enumerate(ctx)
        while chewing_cand_hasNext(ctx) != 0 {
            if let candStr = chewing_cand_String(ctx) {
                candidates.append(String(cString: candStr))
                chewing_free(candStr)
            }
        }
        dbg("candidates() returned \(candidates.count) items")
        return candidates
    }

    override func candidateSelected(_ candidateString: NSAttributedString!) {
        guard let ctx = chewingContext, let session = composingSession else { return }
        guard let client = currentClient else { return }
        let selected = candidateString?.string ?? ""
        dbg("candidateSelected: \(selected)")

        // Find the index of the selected candidate and choose it
        var idx: Int32 = 0
        chewing_cand_Enumerate(ctx)
        while chewing_cand_hasNext(ctx) != 0 {
            if let candStr = chewing_cand_String(ctx) {
                let cand = String(cString: candStr)
                chewing_free(candStr)
                if cand == selected {
                    chewing_cand_choose_by_index(ctx, idx)
                    break
                }
                idx += 1
            }
        }
        updateClientDisplay(ctx: ctx, session: session, client: client)
    }

    // MARK: - Helpers

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

    private func getCandidateCount(_ ctx: OpaquePointer) -> Int {
        var count = 0
        chewing_cand_Enumerate(ctx)
        while chewing_cand_hasNext(ctx) != 0 {
            if let s = chewing_cand_String(ctx) {
                chewing_free(s)
                count += 1
            }
        }
        return count
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
