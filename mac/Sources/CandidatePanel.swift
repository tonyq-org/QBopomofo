import Cocoa

/// Custom candidate window replacing IMKCandidates.
/// All mature macOS input methods (vChewing, McBopomofo, RIME/Squirrel) use custom windows
/// because IMKCandidates' programmatic selection API is broken.
class CandidatePanel: NSPanel {

    static let shared = CandidatePanel()

    // MARK: - State

    private(set) var candidates: [String] = []
    private(set) var highlightedIndex: Int = 0
    private var pageInfo: String = ""

    // MARK: - Views

    private let stackView = NSStackView()
    private let pageLabel = NSTextField(labelWithString: "")
    private var rowViews: [CandidateRowView] = []

    // MARK: - Init

    private init() {
        super.init(
            contentRect: NSRect(x: 0, y: 0, width: 200, height: 100),
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: true
        )
        self.level = .popUpMenu
        self.isOpaque = false
        self.backgroundColor = .clear
        self.hasShadow = true
        self.isMovableByWindowBackground = false

        setupViews()
    }

    required init?(coder: NSCoder) { fatalError() }

    // MARK: - Setup

    private func setupViews() {
        let effectView = NSVisualEffectView()
        effectView.material = .popover
        effectView.state = .active
        effectView.wantsLayer = true
        effectView.layer?.cornerRadius = 6

        stackView.orientation = .vertical
        stackView.alignment = .leading
        stackView.spacing = 0
        stackView.translatesAutoresizingMaskIntoConstraints = false

        pageLabel.font = NSFont.monospacedSystemFont(ofSize: 10, weight: .regular)
        pageLabel.textColor = .secondaryLabelColor
        pageLabel.alignment = .center
        pageLabel.translatesAutoresizingMaskIntoConstraints = false

        effectView.addSubview(stackView)
        effectView.addSubview(pageLabel)
        effectView.translatesAutoresizingMaskIntoConstraints = false

        self.contentView = effectView

        NSLayoutConstraint.activate([
            stackView.topAnchor.constraint(equalTo: effectView.topAnchor, constant: 4),
            stackView.leadingAnchor.constraint(equalTo: effectView.leadingAnchor, constant: 4),
            stackView.trailingAnchor.constraint(equalTo: effectView.trailingAnchor, constant: -4),

            pageLabel.topAnchor.constraint(equalTo: stackView.bottomAnchor, constant: 2),
            pageLabel.leadingAnchor.constraint(equalTo: effectView.leadingAnchor, constant: 4),
            pageLabel.trailingAnchor.constraint(equalTo: effectView.trailingAnchor, constant: -4),
            pageLabel.bottomAnchor.constraint(equalTo: effectView.bottomAnchor, constant: -4),
        ])
    }

    // MARK: - Public API

    /// 選字鍵標籤（由 InputController 根據偏好設定更新）
    var selectionKeyLabels: [String] = ["1","2","3","4","5","6","7","8","9","0"]

    func setCandidates(_ list: [String], page: Int, totalPages: Int) {
        candidates = list
        highlightedIndex = 0
        pageInfo = totalPages > 1 ? "\(page + 1)/\(totalPages)" : ""

        rebuildRows()
        updateHighlight()
        resizeToFit()
    }

    func highlightNext() -> Bool {
        guard highlightedIndex < candidates.count - 1 else { return false }
        highlightedIndex += 1
        updateHighlight()
        return true
    }

    func highlightPrevious() -> Bool {
        guard highlightedIndex > 0 else { return false }
        highlightedIndex -= 1
        updateHighlight()
        return true
    }

    func highlightedCandidate() -> String? {
        guard highlightedIndex < candidates.count else { return nil }
        return candidates[highlightedIndex]
    }

    func show(at point: NSPoint) {
        let panelSize = self.frame.size
        var x = point.x
        var y = point.y - panelSize.height

        // Clamp to screen bounds
        if let screen = NSScreen.main ?? NSScreen.screens.first {
            let screenFrame = screen.visibleFrame

            // Right edge
            if x + panelSize.width > screenFrame.maxX {
                x = screenFrame.maxX - panelSize.width
            }
            // Left edge
            if x < screenFrame.minX {
                x = screenFrame.minX
            }
            // Bottom edge — flip above cursor
            if y < screenFrame.minY {
                y = point.y
            }
            // Top edge
            if y + panelSize.height > screenFrame.maxY {
                y = screenFrame.maxY - panelSize.height
            }
        }

        self.setFrameOrigin(NSPoint(x: x, y: y))
        self.orderFront(nil)
    }

    func hidePanel() {
        self.orderOut(nil)
    }

    var isPanelVisible: Bool {
        return self.isVisible
    }

    // MARK: - Private

    private func rebuildRows() {
        // Remove old rows
        for row in rowViews {
            stackView.removeArrangedSubview(row)
            row.removeFromSuperview()
        }
        rowViews.removeAll()

        // Build new rows
        for (i, cand) in candidates.enumerated() {
            let keyLabel = i < selectionKeyLabels.count ? selectionKeyLabels[i] : ""
            let row = CandidateRowView(keyLabel: keyLabel, candidate: cand)
            stackView.addArrangedSubview(row)
            row.translatesAutoresizingMaskIntoConstraints = false
            row.leadingAnchor.constraint(equalTo: stackView.leadingAnchor).isActive = true
            row.trailingAnchor.constraint(equalTo: stackView.trailingAnchor).isActive = true
            rowViews.append(row)
        }

        pageLabel.stringValue = pageInfo
        pageLabel.isHidden = pageInfo.isEmpty
    }

    private func updateHighlight() {
        for (i, row) in rowViews.enumerated() {
            row.setHighlighted(i == highlightedIndex)
        }
    }

    private func resizeToFit() {
        guard let contentView = self.contentView else { return }
        contentView.layoutSubtreeIfNeeded()
        let fittingSize = contentView.fittingSize
        let width = max(fittingSize.width, 120)
        let height = fittingSize.height
        self.setContentSize(NSSize(width: width, height: height))
    }
}

// MARK: - CandidateRowView

private class CandidateRowView: NSView {
    private let keyLabel: NSTextField
    private let candidateLabel: NSTextField
    private let highlightLayer = CALayer()

    init(keyLabel key: String, candidate: String) {
        keyLabel = NSTextField(labelWithString: key)
        candidateLabel = NSTextField(labelWithString: candidate)
        super.init(frame: .zero)

        wantsLayer = true
        layer?.cornerRadius = 3

        keyLabel.font = NSFont.monospacedSystemFont(ofSize: 14, weight: .regular)
        keyLabel.textColor = .secondaryLabelColor
        keyLabel.setContentHuggingPriority(.required, for: .horizontal)

        candidateLabel.font = NSFont.systemFont(ofSize: 18)
        candidateLabel.textColor = .labelColor
        candidateLabel.lineBreakMode = .byClipping

        let hStack = NSStackView(views: [keyLabel, candidateLabel])
        hStack.orientation = .horizontal
        hStack.spacing = 6
        hStack.alignment = .firstBaseline
        hStack.edgeInsets = NSEdgeInsets(top: 3, left: 6, bottom: 3, right: 10)
        hStack.translatesAutoresizingMaskIntoConstraints = false

        addSubview(hStack)
        NSLayoutConstraint.activate([
            hStack.topAnchor.constraint(equalTo: topAnchor),
            hStack.bottomAnchor.constraint(equalTo: bottomAnchor),
            hStack.leadingAnchor.constraint(equalTo: leadingAnchor),
            hStack.trailingAnchor.constraint(equalTo: trailingAnchor),
        ])
    }

    required init?(coder: NSCoder) { fatalError() }

    func setHighlighted(_ highlighted: Bool) {
        if highlighted {
            layer?.backgroundColor = NSColor.selectedContentBackgroundColor.cgColor
            candidateLabel.textColor = .white
            keyLabel.textColor = NSColor.white.withAlphaComponent(0.7)
        } else {
            layer?.backgroundColor = nil
            candidateLabel.textColor = .labelColor
            keyLabel.textColor = .secondaryLabelColor
        }
    }
}
