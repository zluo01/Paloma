//
//  PalomaPanel.swift
//  Paloma
//

import AppKit

final class PalomaPanel: NSPanel {
    private static let frameName = "PalomaLauncher"
    /// Frame autosave restores the position across launches; only a first run centers.
    private var needsCentering = true

    init(hosting: NSView) {
        super.init(
            contentRect: NSRect(x: 0, y: 0, width: 640, height: 86),
            styleMask: [.nonactivatingPanel, .borderless, .fullSizeContentView],
            backing: .buffered,
            defer: false
        )
        isFloatingPanel = true
        level = .floating
        collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        backgroundColor = .clear
        isOpaque = false
        hasShadow = true
        hidesOnDeactivate = false
        isMovableByWindowBackground = true
        contentView = hosting
        needsCentering = !setFrameUsingName(Self.frameName)
        setFrameAutosaveName(Self.frameName)
    }

    override var canBecomeKey: Bool {
        true
    }

    /// NSPanel's default for an unhandled Escape closes the panel; Escape belongs to the content.
    override func cancelOperation(_: Any?) {}

    /// Dismiss when the user clicks anywhere else, like Spotlight.
    override func resignKey() {
        super.resignKey()
        orderOut(nil)
    }

    /// Every hide path funnels through here (including resignKey above).
    override func orderOut(_ sender: Any?) {
        let wasVisible = isVisible
        super.orderOut(sender)
        if wasVisible {
            NotificationCenter.default.post(name: .panelDidHide, object: self)
        }
    }

    /// Re-anchor so the panel grows downward as the content resizes.
    override func setContentSize(_ size: NSSize) {
        let top = frame.maxY
        super.setContentSize(size)
        setFrameTopLeftPoint(NSPoint(x: frame.origin.x, y: top))
    }

    func show() {
        let cursor = NSEvent.mouseLocation
        let screen = NSScreen.screens.first { NSMouseInRect(cursor, $0.frame, false) }
        if let active = screen ?? NSScreen.main {
            let onActive = active.frame.contains(NSPoint(x: frame.midX, y: frame.midY))
            if needsCentering || !onActive {
                let area = active.visibleFrame
                let top = area.minY + area.height * 0.618 + frame.height / 2
                setFrameTopLeftPoint(NSPoint(x: area.midX - frame.width / 2, y: top))
                needsCentering = false
            }
        }
        makeKeyAndOrderFront(nil)
    }
}
