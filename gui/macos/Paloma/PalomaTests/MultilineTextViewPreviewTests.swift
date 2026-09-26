//
//  MultilineTextViewPreviewTests.swift
//  PalomaTests
//

import AppKit
@testable import Paloma
import Testing

@MainActor
@Suite(.serialized)
final class MultilineTextViewPreviewTests: MultilineTextViewFixture {
    // MARK: - Hit-testing

    @Test func givenImageWhenFindingImageAtItsCenterShouldReturnIt() throws {
        layOut("")
        try pasteImage(width: 400, height: 200)
        let frame = try #require(textView.imageAttachment(at: CGPoint(x: 5, y: 10))?.frame)
        #expect(textView.imageAttachment(at: CGPoint(x: frame.midX, y: frame.midY)) != nil)
    }

    @Test func givenTextBesideImageWhenFindingImageOverTheTextShouldReturnNil() throws {
        layOut("")
        try pasteImage(width: 400, height: 200)
        textView.insertText(" some words after", replacementRange: textView.selectedRange())
        let frame = try #require(textView.imageAttachment(at: CGPoint(x: 5, y: 10))?.frame)
        #expect(textView.imageAttachment(at: CGPoint(x: frame.maxX + 40, y: frame.midY)) == nil)
    }

    @Test func givenSmallImageWhenFindingImageAboveItInTheSameLineShouldReturnNil() throws {
        layOut("")
        try pasteImage(width: 20, height: 10)
        let frame = try #require(textView.imageAttachment(at: CGPoint(x: 5, y: MultilineTextView.lineHeight - 2))?.frame)
        #expect(textView.imageAttachment(at: CGPoint(x: frame.midX, y: frame.minY - 3)) == nil)
    }

    // MARK: - Hover popover

    @Test func givenPointerMovedOverImageWhenPreviewDelayPassesShouldShowPreview() async throws {
        let panel = shownPanel()
        defer { panel.close() }
        try pasteImage(width: 400, height: 200)
        let frame = try #require(textView.imageAttachment(at: CGPoint(x: 5, y: 10))?.frame)

        try movePointer(to: CGPoint(x: frame.midX, y: frame.midY), in: panel)
        try await Task.sleep(for: .milliseconds(500))

        #expect(textView.preview.isShown)
        textView.preview.close()
    }

    @Test func givenPreviewShownWhenPointerMovesOffImageShouldClosePreview() async throws {
        let panel = shownPanel()
        defer { panel.close() }
        try pasteImage(width: 400, height: 200)
        textView.insertText(" some words after", replacementRange: textView.selectedRange())
        let frame = try #require(textView.imageAttachment(at: CGPoint(x: 5, y: 10))?.frame)
        try movePointer(to: CGPoint(x: frame.midX, y: frame.midY), in: panel)
        try await Task.sleep(for: .milliseconds(500))
        try #require(textView.preview.isShown)

        try movePointer(to: CGPoint(x: frame.maxX + 40, y: frame.midY), in: panel)
        for _ in 0 ..< 10 where textView.preview.isShown {
            try await Task.sleep(for: .milliseconds(100))
        }

        #expect(!textView.preview.isShown)
    }

    @Test func givenLargeImageWhenShowingPreviewShouldFitWithinPreviewBounds() async throws {
        let panel = shownPanel()
        defer { panel.close() }
        try pasteImage(width: 1200, height: 400)
        let frame = try #require(textView.imageAttachment(at: CGPoint(x: 5, y: 10))?.frame)

        try movePointer(to: CGPoint(x: frame.midX, y: frame.midY), in: panel)
        try await Task.sleep(for: .milliseconds(500))

        #expect(textView.preview.contentSize == CGSize(width: 360, height: 120))
        textView.preview.close()
    }

    // MARK: - Helpers

    private func shownPanel() -> NSPanel {
        let panel = NSPanel(contentRect: textView.frame, styleMask: [.titled, .nonactivatingPanel], backing: .buffered, defer: false)
        panel.contentView = textView
        panel.orderFront(nil)
        return panel
    }

    private func movePointer(to point: CGPoint, in panel: NSPanel) throws {
        let event = try #require(NSEvent.mouseEvent(
            with: .mouseMoved,
            location: textView.convert(point, to: nil),
            modifierFlags: [],
            timestamp: 0,
            windowNumber: panel.windowNumber,
            context: nil,
            eventNumber: 0,
            clickCount: 0,
            pressure: 0
        ))
        textView.mouseMoved(with: event)
    }
}
