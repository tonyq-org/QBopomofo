import Cocoa

/// QBopomofo 偏好設定視窗
class PreferencesWindow: NSWindow {

    static let shared = PreferencesWindow()

    private init() {
        super.init(
            contentRect: NSRect(x: 0, y: 0, width: 400, height: 376),
            styleMask: [.titled, .closable],
            backing: .buffered,
            defer: false
        )
        title = "Q注音 偏好設定"
        isReleasedWhenClosed = false
        center()
        setupUI()
    }

    private func setupUI() {
        let contentView = NSView(frame: NSRect(x: 0, y: 0, width: 400, height: 376))

        var y = 326

        // Title
        let titleLabel = NSTextField(labelWithString: "Q注音 設定")
        titleLabel.font = .boldSystemFont(ofSize: 16)
        titleLabel.frame = NSRect(x: 20, y: y, width: 360, height: 24)
        contentView.addSubview(titleLabel)
        y -= 40

        // Candidates per page
        let candLabel = NSTextField(labelWithString: "每頁候選字數量：")
        candLabel.frame = NSRect(x: 20, y: y, width: 140, height: 22)
        contentView.addSubview(candLabel)

        let candPopup = NSPopUpButton(frame: NSRect(x: 170, y: y - 2, width: 80, height: 26))
        candPopup.addItems(withTitles: ["5", "7", "9"])
        let currentCand = UserDefaults.standard.integer(forKey: "org.qbopomofo.candPerPage")
        let candValue = currentCand > 0 ? currentCand : 9
        candPopup.selectItem(withTitle: "\(candValue)")
        candPopup.target = self
        candPopup.action = #selector(candPerPageChanged(_:))
        contentView.addSubview(candPopup)
        y -= 36

        // Shift behavior
        let shiftLabel = NSTextField(labelWithString: "Shift 鍵行為：")
        shiftLabel.frame = NSRect(x: 20, y: y, width: 140, height: 22)
        contentView.addSubview(shiftLabel)

        let shiftPopup = NSPopUpButton(frame: NSRect(x: 170, y: y - 2, width: 180, height: 26))
        shiftPopup.addItems(withTitles: ["SmartToggle（按住英文、放開中文）", "傳統切換"])
        let shiftMode = UserDefaults.standard.integer(forKey: "org.qbopomofo.shiftBehavior")
        shiftPopup.selectItem(at: shiftMode == 0 ? 0 : shiftMode - 1)
        shiftPopup.target = self
        shiftPopup.action = #selector(shiftBehaviorChanged(_:))
        contentView.addSubview(shiftPopup)
        y -= 36

        // Selection keys
        let selLabel = NSTextField(labelWithString: "選字鍵：")
        selLabel.frame = NSRect(x: 20, y: y, width: 140, height: 22)
        contentView.addSubview(selLabel)

        let selPopup = NSPopUpButton(frame: NSRect(x: 170, y: y - 2, width: 180, height: 26))
        selPopup.addItems(withTitles: ["1234567890", "asdfghjkl;"])
        let currentSel = UserDefaults.standard.string(forKey: "org.qbopomofo.selectionKeys") ?? "1234567890"
        selPopup.selectItem(withTitle: currentSel)
        selPopup.target = self
        selPopup.action = #selector(selectionKeysChanged(_:))
        contentView.addSubview(selPopup)
        y -= 36

        // CapsLock behavior
        let capsLabel = NSTextField(labelWithString: "CapsLock 行為：")
        capsLabel.frame = NSRect(x: 20, y: y, width: 140, height: 22)
        contentView.addSubview(capsLabel)

        let capsPopup = NSPopUpButton(frame: NSRect(x: 170, y: y - 2, width: 180, height: 26))
        capsPopup.addItems(withTitles: ["切換英文模式", "不處理"])
        let capsMode = UserDefaults.standard.integer(forKey: "org.qbopomofo.capsLockBehavior")
        capsPopup.selectItem(at: capsMode)
        capsPopup.target = self
        capsPopup.action = #selector(capsLockChanged(_:))
        contentView.addSubview(capsPopup)
        y -= 36

        // Space cycle count
        let cycleLabel = NSTextField(labelWithString: "空白鍵自動選字：")
        cycleLabel.frame = NSRect(x: 20, y: y, width: 140, height: 22)
        contentView.addSubview(cycleLabel)

        let cyclePopup = NSPopUpButton(frame: NSRect(x: 170, y: y - 2, width: 180, height: 26))
        cyclePopup.addItems(withTitles: ["0（直接開啟候選字）", "1 次", "2 次", "3 次"])
        let currentCycle = UserDefaults.standard.integer(forKey: "org.qbopomofo.spaceCycleCount")
        cyclePopup.selectItem(at: min(max(currentCycle, 0), 3))
        cyclePopup.target = self
        cyclePopup.action = #selector(spaceCycleCountChanged(_:))
        contentView.addSubview(cyclePopup)
        y -= 36

        // Persistent logging
        let logCheck = NSButton(checkboxWithTitle: "保留偵錯紀錄（/tmp/qbopomofo-*.log）", target: self, action: #selector(persistentLogChanged(_:)))
        logCheck.frame = NSRect(x: 20, y: y, width: 360, height: 22)
        logCheck.state = UserDefaults.standard.bool(forKey: "org.qbopomofo.persistentLog") ? .on : .off
        contentView.addSubview(logCheck)
        y -= 50

        // Version info
        let versionLabel = NSTextField(labelWithString: "版本：0.1.0（build: \(kBuildTimestamp)）")
        versionLabel.font = .systemFont(ofSize: 11)
        versionLabel.textColor = .secondaryLabelColor
        versionLabel.frame = NSRect(x: 20, y: 20, width: 360, height: 18)
        contentView.addSubview(versionLabel)

        self.contentView = contentView
    }

    @objc private func candPerPageChanged(_ sender: NSPopUpButton) {
        if let title = sender.titleOfSelectedItem, let value = Int(title) {
            UserDefaults.standard.set(value, forKey: "org.qbopomofo.candPerPage")
            NotificationCenter.default.post(name: .qbopomofoPreferencesChanged, object: nil)
        }
    }

    @objc private func shiftBehaviorChanged(_ sender: NSPopUpButton) {
        let value = sender.indexOfSelectedItem + 1 // 1=SmartToggle, 2=Traditional
        UserDefaults.standard.set(value, forKey: "org.qbopomofo.shiftBehavior")
        NotificationCenter.default.post(name: .qbopomofoPreferencesChanged, object: nil)
    }

    @objc private func selectionKeysChanged(_ sender: NSPopUpButton) {
        if let title = sender.titleOfSelectedItem {
            UserDefaults.standard.set(title, forKey: "org.qbopomofo.selectionKeys")
            NotificationCenter.default.post(name: .qbopomofoPreferencesChanged, object: nil)
        }
    }

    @objc private func capsLockChanged(_ sender: NSPopUpButton) {
        UserDefaults.standard.set(sender.indexOfSelectedItem, forKey: "org.qbopomofo.capsLockBehavior")
        NotificationCenter.default.post(name: .qbopomofoPreferencesChanged, object: nil)
    }

    @objc private func spaceCycleCountChanged(_ sender: NSPopUpButton) {
        UserDefaults.standard.set(sender.indexOfSelectedItem, forKey: "org.qbopomofo.spaceCycleCount")
        NotificationCenter.default.post(name: .qbopomofoPreferencesChanged, object: nil)
    }

    @objc private func persistentLogChanged(_ sender: NSButton) {
        UserDefaults.standard.set(sender.state == .on, forKey: "org.qbopomofo.persistentLog")
    }

    func showWindow() {
        makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }
}

extension Notification.Name {
    static let qbopomofoPreferencesChanged = Notification.Name("org.qbopomofo.preferencesChanged")
}
