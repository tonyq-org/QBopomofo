import Cocoa
import InputMethodKit
import CChewing

/// QBopomofo 的核心輸入控制器
/// 負責處理按鍵事件、與 libchewing 引擎互動、管理輸入狀態
/// 組字邏輯（Shift SmartToggle、中英混排）委託給 Rust ComposingSession (qb_composing_*)
@objc(QBopomofoInputController)
class QBopomofoInputController: IMKInputController {

    // MARK: - Properties

    private var chewingContext: OpaquePointer?
    private var composingSession: OpaquePointer?
    private var currentClient: IMKTextInput?

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
        if let dictPath = Bundle.main.resourcePath {
            setenv("CHEWING_PATH", dictPath, 1)
        }

        chewingContext = chewing_new()
        guard chewingContext != nil else {
            NSLog("QBopomofo: Failed to create chewing context")
            return
        }

        composingSession = qb_composing_new()
        qb_composing_set_shift_behavior(composingSession, 1) // SmartToggle

        chewing_set_candPerPage(chewingContext, 9)
        chewing_set_maxChiSymbolLen(chewingContext, 20)
        chewing_set_spaceAsSelection(chewingContext, 1)
        chewing_set_escCleanAllBuf(chewingContext, 1)
        chewing_set_autoShiftCur(chewingContext, 1)

        NSLog("QBopomofo: Engine initialized")
    }

    // MARK: - IMKStateSetting

    override func activateServer(_ sender: Any!) {
        currentClient = sender as? IMKTextInput
        if chewingContext == nil { initializeEngine() }
        NSLog("QBopomofo: Server activated")
    }

    override func deactivateServer(_ sender: Any!) {
        commitComposition(sender)
        currentClient = nil
        NSLog("QBopomofo: Server deactivated")
    }

    // MARK: - IMKServerInput

    override func recognizedEvents(_ sender: Any!) -> Int {
        let events: NSEvent.EventTypeMask = [.keyDown, .flagsChanged]
        return Int(events.rawValue)
    }

    override func handle(_ event: NSEvent!, client sender: Any!) -> Bool {
        guard let event = event else { return false }
        guard let ctx = chewingContext, let session = composingSession else { return false }
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

        // Pass through Command/Control
        if modifiers.contains(.command) || modifiers.contains(.control) { return false }

        // Shift held + typing → temporary English
        if shift && qb_composing_is_shift_held(session) != 0 {
            if let ch = chars.first, ch.isASCII {
                qb_composing_type_english(session, UInt8(ch.asciiValue ?? 0))
                updateClientDisplay(ctx: ctx, session: session, client: client)
                return true
            }
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
                commitAll(ctx: ctx, session: session, client: client)
                return true
            }
            if keyCode == 51 { // Backspace
                if qb_composing_backspace_english(session) != 0 {
                    updateClientDisplay(ctx: ctx, session: session, client: client)
                    return true
                }
            }
            if let ch = chars.first, ch.isASCII, !ch.isNewline {
                qb_composing_type_english(session, UInt8(ch.asciiValue ?? 0))
                updateClientDisplay(ctx: ctx, session: session, client: client)
                return true
            }
        }

        // Enter/Escape with mixed content
        if keyCode == 36 && qb_composing_has_mixed_content(session) != 0 {
            commitAll(ctx: ctx, session: session, client: client)
            return true
        }
        if keyCode == 53 && qb_composing_has_mixed_content(session) != 0 {
            qb_composing_clear(session)
            chewing_handle_Esc(ctx)
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
        switch keyCode {
        case 36: chewing_handle_Enter(ctx); return true
        case 51: chewing_handle_Backspace(ctx); return true
        case 53: chewing_handle_Esc(ctx); return true
        case 49: chewing_handle_Space(ctx); return true
        case 48: chewing_handle_Tab(ctx); return true
        case 117: chewing_handle_Del(ctx); return true
        case 123: chewing_handle_Left(ctx); return true
        case 124: chewing_handle_Right(ctx); return true
        case 125: chewing_handle_Down(ctx); return true
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
                client.insertText(String(cString: commitStr), replacementRange: NSRange(location: NSNotFound, length: 0))
                chewing_free(commitStr)
            }
        }

        // Build display via Rust session (handles mixed Chinese/English)
        let chinese = getChewingBuffer(ctx)
        let bopomofo = getBopomofoReading(ctx)
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
    }

    // MARK: - Commit

    private func commitAll(ctx: OpaquePointer, session: OpaquePointer, client: IMKTextInput) {
        var finalChinese = ""
        if chewing_buffer_Len(ctx) > 0 {
            chewing_handle_Enter(ctx)
            if chewing_commit_Check(ctx) != 0 {
                if let commitStr = chewing_commit_String(ctx) {
                    finalChinese = String(cString: commitStr)
                    chewing_free(commitStr)
                }
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
            client.insertText(result, replacementRange: NSRange(location: NSNotFound, length: 0))
        }
        client.setMarkedText("", selectionRange: NSRange(location: 0, length: 0), replacementRange: NSRange(location: NSNotFound, length: 0))
    }

    override func commitComposition(_ sender: Any!) {
        guard let ctx = chewingContext, let session = composingSession else { return }
        guard let client = sender as? IMKTextInput else { return }
        commitAll(ctx: ctx, session: session, client: client)
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
        return candidates
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

    private func getBopomofoReading(_ ctx: OpaquePointer) -> String {
        if chewing_bopomofo_Check(ctx) != 0, let bopoStr = chewing_bopomofo_String(ctx) {
            let s = String(cString: bopoStr)
            chewing_free(bopoStr)
            return s
        }
        return ""
    }
}
