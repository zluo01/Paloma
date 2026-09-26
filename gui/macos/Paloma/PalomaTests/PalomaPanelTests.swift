//
//  PalomaPanelTests.swift
//  PalomaTests
//

import AppKit
@testable import Paloma
import Testing

private final class FocusableView: NSView {
    override var acceptsFirstResponder: Bool {
        true
    }
}

@MainActor
struct PalomaPanelTests {
    private let panel: PalomaPanel
    private let textView: ComposerTextView
    private let other: FocusableView

    init() {
        let content = NSView(frame: NSRect(x: 0, y: 0, width: 640, height: 80))
        textView = ComposerTextView.make()
        textView.frame = NSRect(x: 0, y: 0, width: 640, height: 40)
        other = FocusableView(frame: NSRect(x: 0, y: 40, width: 640, height: 40))
        content.addSubview(textView)
        content.addSubview(other)
        let query = QueryModel()
        query.textView = textView
        panel = PalomaPanel(hosting: content, query: query)
        panel.setFrameAutosaveName("")
    }

    @Test func givenFocusLeftTheTextViewWhenTypingShouldRefocusTextViewAndInsert() throws {
        try #require(panel.makeFirstResponder(other))

        try panel.sendEvent(key("a", keyCode: 0))

        #expect(panel.firstResponder === textView)
        #expect(textView.string == "a")
    }

    @Test func givenFocusLeftTheTextViewWhenPressingACommandChordShouldRefocusTextView() throws {
        try #require(panel.makeFirstResponder(other))

        try panel.sendEvent(key("y", keyCode: 16, modifiers: .command))

        #expect(panel.firstResponder === textView)
    }

    @Test func givenFocusLeftTheTextViewWhenPressingCommandCShouldLeaveFocusWhereItIs() throws {
        try #require(panel.makeFirstResponder(other))

        try panel.sendEvent(key("c", keyCode: 8, modifiers: .command))

        #expect(panel.firstResponder === other)
    }

    @Test func givenFocusLeftTheTextViewWhenCommandVArrivesAsKeyEquivalentShouldRefocusTextView() throws {
        try #require(panel.makeFirstResponder(other))

        _ = try panel.performKeyEquivalent(with: key("v", keyCode: 9, modifiers: .command))

        #expect(panel.firstResponder === textView)
    }

    @Test func givenFocusLeftTheTextViewWhenCommandCArrivesAsKeyEquivalentShouldLeaveFocusWhereItIs() throws {
        try #require(panel.makeFirstResponder(other))

        _ = try panel.performKeyEquivalent(with: key("c", keyCode: 8, modifiers: .command))

        #expect(panel.firstResponder === other)
    }

    @Test func givenTextViewFocusedWhenTypingShouldKeepFocusAndInsert() throws {
        try #require(panel.makeFirstResponder(textView))

        try panel.sendEvent(key("a", keyCode: 0))

        #expect(panel.firstResponder === textView)
        #expect(textView.string == "a")
    }

    @Test func givenFocusLeftTheTextViewWhenMouseEventArrivesShouldNotChangeFocus() throws {
        try #require(panel.makeFirstResponder(other))

        let click = try #require(NSEvent.mouseEvent(
            with: .leftMouseDown, location: NSPoint(x: 320, y: 60), modifierFlags: [], timestamp: 0,
            windowNumber: panel.windowNumber, context: nil, eventNumber: 1, clickCount: 1, pressure: 1
        ))
        panel.sendEvent(click)

        #expect(panel.firstResponder === other)
    }

    private func key(_ characters: String, keyCode: UInt16, modifiers: NSEvent.ModifierFlags = []) throws -> NSEvent {
        try #require(NSEvent.keyEvent(
            with: .keyDown, location: .zero, modifierFlags: modifiers, timestamp: 0,
            windowNumber: panel.windowNumber, context: nil,
            characters: characters, charactersIgnoringModifiers: characters, isARepeat: false, keyCode: keyCode
        ))
    }
}
