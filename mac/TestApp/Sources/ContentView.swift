import SwiftUI

/// QBopomofo 模擬輸入框
///
/// 模擬真實輸入法的完整流程：
/// 1. 攔截鍵盤輸入
/// 2. 送給 chewing 引擎
/// 3. 顯示注音組字區
/// 4. 顯示候選字列表
/// 5. 送字到文字輸出區
struct ContentView: View {
    @StateObject private var engine = ChewingBridge()
    @State private var showSettings = false

    var body: some View {
        VStack(spacing: 0) {
            // Title + Mode Picker + Settings
            HStack {
                Text("QBopomofo 模擬輸入框")
                    .font(.headline)
                Spacer()

                // 中/英 狀態指示
                Text(engine.isEnglishMode ? "英" : "中")
                    .font(.system(size: 14, weight: .bold, design: .rounded))
                    .foregroundStyle(engine.isEnglishMode ? .orange : .blue)
                    .frame(width: 28, height: 28)
                    .background(
                        (engine.isEnglishMode ? Color.orange : Color.blue).opacity(0.15)
                    )
                    .cornerRadius(6)

                Picker("模式", selection: Binding(
                    get: { engine.currentMode },
                    set: { engine.switchMode($0) }
                )) {
                    ForEach(TypingModeSwift.allCases) { mode in
                        Text(mode.displayName).tag(mode)
                    }
                }
                .pickerStyle(.menu)
                .frame(width: 140)

                // Settings button (simulates future status tray menu)
                Button(action: { showSettings.toggle() }) {
                    Image(systemName: "gearshape.fill")
                }
                .popover(isPresented: $showSettings, arrowEdge: .bottom) {
                    ModeSettingsView(engine: engine)
                }

                Button("清除") { engine.reset() }
                    .keyboardShortcut("r", modifiers: .command)
            }
            .padding()

            Divider()

            // Main content
            HSplitView {
                // Left: Input simulation
                VStack(alignment: .leading, spacing: 12) {
                    // Output text area (committed text)
                    VStack(alignment: .leading, spacing: 4) {
                        Text("輸出文字")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        AppKitTextView(text: $engine.committedText, isEditable: false, font: .systemFont(ofSize: 18))
                            .frame(maxWidth: .infinity, minHeight: 60, maxHeight: 120)
                            .border(.separator)
                    }

                    // Pre-edit display (composing + bopomofo + inline English)
                    VStack(alignment: .leading, spacing: 4) {
                        Text("組字區")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        HStack(spacing: 0) {
                            if !engine.preEditDisplay.isEmpty {
                                Text(engine.preEditDisplay)
                                    .font(.system(size: 20))
                                    .underline()
                                    .foregroundStyle(.primary)
                            } else {
                                Text("（等待輸入）")
                                    .font(.system(size: 16))
                                    .foregroundStyle(.tertiary)
                            }
                        }
                        .frame(maxWidth: .infinity, minHeight: 36, alignment: .leading)
                        .padding(8)
                        .background(Color.blue.opacity(0.05))
                        .border(Color.blue.opacity(0.3))
                        .onChange(of: engine.preEditDisplay) {
                            guard engine.lastKeyTime > 0 else { return }
                            let elapsed = (CFAbsoluteTimeGetCurrent() - engine.lastKeyTime) * 1000
                            engine.lastKeyTime = 0
                            engine.logRender(elapsed)
                        }
                    }

                    // Candidates
                    if engine.showCandidates {
                        VStack(alignment: .leading, spacing: 4) {
                            Text("候選字")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                            HStack(spacing: 8) {
                                ForEach(Array(engine.candidates.prefix(9).enumerated()), id: \.offset) { index, candidate in
                                    HStack(spacing: 2) {
                                        Text("\(index + 1)")
                                            .font(.caption)
                                            .foregroundStyle(.secondary)
                                        Text(candidate)
                                            .font(.system(size: 18))
                                    }
                                    .padding(.horizontal, 6)
                                    .padding(.vertical, 4)
                                    .background(
                                        index == engine.selectedCandidateIndex
                                            ? Color.accentColor.opacity(0.2)
                                            : Color.clear
                                    )
                                    .cornerRadius(4)
                                }
                            }
                            .padding(8)
                            .background(.background)
                            .border(.separator)
                        }
                    }

                    // Keyboard input area
                    VStack(alignment: .leading, spacing: 4) {
                        Text("鍵盤輸入（點擊下方區域後開始打字）")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        KeyCaptureView(engine: engine)
                            .frame(maxWidth: .infinity, minHeight: 40)
                            .background(Color.green.opacity(0.05))
                            .border(Color.green.opacity(0.3))
                    }

                    Spacer()
                }
                .padding()
                .frame(minWidth: 350)

                // Right: Debug log (AppKit NSTextView for performance)
                VStack(alignment: .leading, spacing: 4) {
                    HStack {
                        Text("引擎日誌")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        Spacer()
                        Button("複製日誌") {
                            NSPasteboard.general.clearContents()
                            NSPasteboard.general.setString(engine.debugLog, forType: .string)
                        }
                        .font(.caption)
                        Button("清除日誌") {
                            engine.clearLog()
                        }
                        .font(.caption)
                    }
                    .padding(.top, 12)
                    .padding(.horizontal, 8)

                    AppKitTextView(
                        text: $engine.debugLog,
                        isEditable: false,
                        font: .monospacedSystemFont(ofSize: 11, weight: .regular),
                        textColor: .secondaryLabelColor,
                        scrollToBottom: true
                    )
                    .border(.separator)
                    .padding(.horizontal, 8)
                    .padding(.bottom, 12)
                }
                .frame(minWidth: 250)
            }
        }
    }
}

// MARK: - AppKit NSTextView wrapper (high-performance text display)

struct AppKitTextView: NSViewRepresentable {
    @Binding var text: String
    var isEditable: Bool = false
    var font: NSFont = .systemFont(ofSize: 13)
    var textColor: NSColor = .labelColor
    var scrollToBottom: Bool = false

    func makeNSView(context: Context) -> NSScrollView {
        let scrollView = NSTextView.scrollableTextView()
        let textView = scrollView.documentView as! NSTextView
        textView.isEditable = isEditable
        textView.isSelectable = true
        textView.font = font
        textView.textColor = textColor
        textView.backgroundColor = .textBackgroundColor
        textView.textContainerInset = NSSize(width: 4, height: 4)
        textView.isAutomaticQuoteSubstitutionEnabled = false
        textView.isAutomaticDashSubstitutionEnabled = false
        textView.isAutomaticTextReplacementEnabled = false
        context.coordinator.textView = textView
        context.coordinator.scrollView = scrollView
        return scrollView
    }

    func updateNSView(_ scrollView: NSScrollView, context: Context) {
        guard let textView = scrollView.documentView as? NSTextView else { return }
        if textView.string != text {
            textView.string = text
            if scrollToBottom {
                textView.scrollToEndOfDocument(nil)
            }
        }
    }

    func makeCoordinator() -> Coordinator { Coordinator() }

    class Coordinator {
        weak var textView: NSTextView?
        weak var scrollView: NSScrollView?
    }
}

// MARK: - Mode Settings Popover (simulates future status tray settings)

struct ModeSettingsView: View {
    @ObservedObject var engine: ChewingBridge

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            // Header
            HStack {
                Image(systemName: "gearshape.fill")
                Text("\(engine.currentMode.displayName) 設定")
                    .font(.headline)
            }

            Divider()

            // Shift behavior
            HStack {
                Text("Shift 鍵")
                    .frame(width: 80, alignment: .trailing)
                Picker("", selection: Binding(
                    get: { engine.shiftBehavior },
                    set: {
                        engine.shiftBehavior = $0
                        engine.applyPreferences()
                    }
                )) {
                    ForEach(ShiftBehaviorSwift.allCases) { behavior in
                        Text(behavior.displayName).tag(behavior)
                    }
                }
                .pickerStyle(.menu)
                .frame(width: 160)
            }

            // CapsLock behavior
            HStack {
                Text("Caps Lock")
                    .frame(width: 80, alignment: .trailing)
                Picker("", selection: Binding(
                    get: { engine.capsLockBehavior },
                    set: {
                        engine.capsLockBehavior = $0
                        engine.applyPreferences()
                    }
                )) {
                    ForEach(CapsLockBehaviorSwift.allCases) { behavior in
                        Text(behavior.displayName).tag(behavior)
                    }
                }
                .pickerStyle(.menu)
                .frame(width: 160)
            }

            Divider()

            // Candidates per page
            HStack {
                Text("每頁候選字")
                    .frame(width: 80, alignment: .trailing)
                Picker("", selection: Binding(
                    get: { engine.candidatesPerPage },
                    set: {
                        engine.candidatesPerPage = $0
                        engine.applyPreferences()
                    }
                )) {
                    ForEach([5, 7, 9, 10], id: \.self) { n in
                        Text("\(n)").tag(n)
                    }
                }
                .pickerStyle(.menu)
                .frame(width: 80)
            }

            // Toggles
            Toggle("Space 選字", isOn: Binding(
                get: { engine.spaceAsSelection },
                set: {
                    engine.spaceAsSelection = $0
                    engine.applyPreferences()
                }
            ))
            .padding(.leading, 84)

            Toggle("Esc 清除全部", isOn: Binding(
                get: { engine.escClearAll },
                set: {
                    engine.escClearAll = $0
                    engine.applyPreferences()
                }
            ))
            .padding(.leading, 84)

            Toggle("自動學習詞彙", isOn: Binding(
                get: { engine.autoLearn },
                set: {
                    engine.autoLearn = $0
                    engine.applyPreferences()
                }
            ))
            .padding(.leading, 84)
        }
        .padding()
        .frame(width: 300)
    }
}

// MARK: - Key Capture View (NSView wrapper to intercept keyDown)

struct KeyCaptureView: NSViewRepresentable {
    let engine: ChewingBridge

    func makeNSView(context: Context) -> KeyCaptureNSView {
        let view = KeyCaptureNSView()
        view.engine = engine
        return view
    }

    func updateNSView(_ nsView: KeyCaptureNSView, context: Context) {
        nsView.engine = engine
    }
}

class KeyCaptureNSView: NSView {
    var engine: ChewingBridge?

    override var acceptsFirstResponder: Bool { true }

    override func keyDown(with event: NSEvent) {
        guard let engine = engine else { return }

        let chars = event.characters ?? ""
        let shift = event.modifierFlags.contains(.shift)

        // Pass through Command+key
        if event.modifierFlags.contains(.command) {
            super.keyDown(with: event)
            return
        }

        Task { @MainActor in
            let handled = engine.handleKey(
                keyCode: event.keyCode,
                characters: chars,
                shift: shift
            )
            if !handled {
                super.keyDown(with: event)
            }
        }
    }

    override func flagsChanged(with event: NSEvent) {
        guard let engine = engine else { return }
        let isShiftDown = event.modifierFlags.contains(.shift)
        Task { @MainActor in
            engine.handleShiftToggle(isShiftDown: isShiftDown)
        }
    }

    override func draw(_ dirtyRect: NSRect) {
        super.draw(dirtyRect)
        // Draw focus indicator
        if window?.firstResponder == self {
            NSColor.systemGreen.withAlphaComponent(0.1).setFill()
            dirtyRect.fill()
            let text = "✓ 已取得鍵盤焦點 — 開始打字"
            let attrs: [NSAttributedString.Key: Any] = [
                .font: NSFont.systemFont(ofSize: 13),
                .foregroundColor: NSColor.secondaryLabelColor,
            ]
            let size = text.size(withAttributes: attrs)
            let point = NSPoint(
                x: (dirtyRect.width - size.width) / 2,
                y: (dirtyRect.height - size.height) / 2
            )
            text.draw(at: point, withAttributes: attrs)
        } else {
            let text = "點擊此處取得鍵盤焦點"
            let attrs: [NSAttributedString.Key: Any] = [
                .font: NSFont.systemFont(ofSize: 13),
                .foregroundColor: NSColor.tertiaryLabelColor,
            ]
            let size = text.size(withAttributes: attrs)
            let point = NSPoint(
                x: (dirtyRect.width - size.width) / 2,
                y: (dirtyRect.height - size.height) / 2
            )
            text.draw(at: point, withAttributes: attrs)
        }
    }

    override func mouseDown(with event: NSEvent) {
        window?.makeFirstResponder(self)
        needsDisplay = true
    }

    override func becomeFirstResponder() -> Bool {
        needsDisplay = true
        return true
    }

    override func resignFirstResponder() -> Bool {
        needsDisplay = true
        return true
    }
}
