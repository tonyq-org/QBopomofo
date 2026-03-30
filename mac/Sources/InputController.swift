import Cocoa
import InputMethodKit
import CChewing

/// QBopomofo 的核心輸入控制器
/// 負責處理按鍵事件、與 libchewing 引擎互動、管理輸入狀態
@objc(QBopomofoInputController)
class QBopomofoInputController: IMKInputController {

    // MARK: - Properties

    private var chewingContext: OpaquePointer?
    private var currentClient: IMKTextInput?

    // MARK: - Lifecycle

    override init!(server: IMKServer!, delegate: Any!, client inputClient: Any!) {
        super.init(server: server, delegate: delegate, client: inputClient)
        initializeEngine()
    }

    deinit {
        if let ctx = chewingContext {
            chewing_delete(ctx)
        }
    }

    private func initializeEngine() {
        // Set dictionary data path to bundle resources
        if let dictPath = Bundle.main.resourcePath {
            setenv("CHEWING_PATH", dictPath, 1)
        }

        chewingContext = chewing_new()
        guard chewingContext != nil else {
            NSLog("QBopomofo: Failed to create chewing context")
            return
        }

        // Configure engine defaults
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
        if chewingContext == nil {
            initializeEngine()
        }
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
        guard let event = event, event.type == .keyDown else {
            return false
        }
        guard let ctx = chewingContext else {
            return false
        }
        guard let client = sender as? IMKTextInput else {
            return false
        }

        let keyCode = event.keyCode
        let chars = event.characters ?? ""
        let modifiers = event.modifierFlags

        // Pass through if Command or Control is held (system shortcuts)
        if modifiers.contains(.command) || modifiers.contains(.control) {
            return false
        }

        let handled = processKey(ctx: ctx, keyCode: keyCode, chars: chars, modifiers: modifiers)

        if handled {
            updateClientDisplay(ctx: ctx, client: client)
        }

        return handled
    }

    // MARK: - Key Processing

    private func processKey(ctx: OpaquePointer, keyCode: UInt16,
                            chars: String, modifiers: NSEvent.ModifierFlags) -> Bool {
        // Special keys
        switch keyCode {
        case 36: // Return
            chewing_handle_Enter(ctx)
            return true
        case 51: // Backspace
            chewing_handle_Backspace(ctx)
            return true
        case 53: // Escape
            chewing_handle_Esc(ctx)
            return true
        case 49: // Space
            chewing_handle_Space(ctx)
            return true
        case 48: // Tab
            chewing_handle_Tab(ctx)
            return true
        case 117: // Delete (Forward)
            chewing_handle_Del(ctx)
            return true
        case 123: // Left
            chewing_handle_Left(ctx)
            return true
        case 124: // Right
            chewing_handle_Right(ctx)
            return true
        case 125: // Down
            chewing_handle_Down(ctx)
            return true
        case 126: // Up
            chewing_handle_Up(ctx)
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

        // Regular character input
        guard let firstChar = chars.first else {
            return false
        }

        let charCode = Int32(firstChar.asciiValue ?? 0)
        if charCode > 0 {
            chewing_handle_Default(ctx, charCode)
            return true
        }

        return false
    }

    // MARK: - Client Display Update

    private func updateClientDisplay(ctx: OpaquePointer, client: IMKTextInput) {
        // Check if there's committed text
        if chewing_commit_Check(ctx) != 0 {
            if let commitStr = chewing_commit_String(ctx) {
                let text = String(cString: commitStr)
                client.insertText(text, replacementRange: NSRange(location: NSNotFound, length: 0))
                chewing_free(commitStr)
            }
        }

        // Update composing buffer (pre-edit text)
        let bufferLen = chewing_buffer_Len(ctx)
        if bufferLen > 0 {
            if let bufferStr = chewing_buffer_String(ctx) {
                let composing = String(cString: bufferStr)
                // Show bopomofo reading if available
                var display = composing
                if chewing_bopomofo_Check(ctx) != 0,
                   let bopoStr = chewing_bopomofo_String(ctx) {
                    display += String(cString: bopoStr)
                    chewing_free(bopoStr)
                }

                client.setMarkedText(
                    display,
                    selectionRange: NSRange(location: display.count, length: 0),
                    replacementRange: NSRange(location: NSNotFound, length: 0)
                )
                chewing_free(bufferStr)
            }
        } else if chewing_bopomofo_Check(ctx) != 0 {
            // Only bopomofo input, no composed text yet
            if let bopoStr = chewing_bopomofo_String(ctx) {
                let reading = String(cString: bopoStr)
                client.setMarkedText(
                    reading,
                    selectionRange: NSRange(location: reading.count, length: 0),
                    replacementRange: NSRange(location: NSNotFound, length: 0)
                )
                chewing_free(bopoStr)
            }
        } else {
            // Clear marked text
            client.setMarkedText(
                "",
                selectionRange: NSRange(location: 0, length: 0),
                replacementRange: NSRange(location: NSNotFound, length: 0)
            )
        }
    }

    // MARK: - Composition Management

    override func commitComposition(_ sender: Any!) {
        guard let ctx = chewingContext else { return }
        guard let client = sender as? IMKTextInput else { return }

        // Force commit current buffer
        chewing_handle_Enter(ctx)
        if chewing_commit_Check(ctx) != 0 {
            if let commitStr = chewing_commit_String(ctx) {
                let text = String(cString: commitStr)
                client.insertText(text, replacementRange: NSRange(location: NSNotFound, length: 0))
                chewing_free(commitStr)
            }
        }

        // Clear marked text
        client.setMarkedText(
            "",
            selectionRange: NSRange(location: 0, length: 0),
            replacementRange: NSRange(location: NSNotFound, length: 0)
        )

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
}
