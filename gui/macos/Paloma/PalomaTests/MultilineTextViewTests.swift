//
//  MultilineTextViewTests.swift
//  PalomaTests
//

import AppKit
@testable import Paloma
import Testing

@MainActor
private final class TextChangeRecorder: NSObject, NSTextViewDelegate {
    private(set) var changes = 0

    func textDidChange(_: Notification) {
        changes += 1
    }
}

@MainActor
final class MultilineTextViewTests: MultilineTextViewFixture {
    // MARK: - isOnEdge

    @Test func givenEmptyTextWhenMovingUpShouldBeOnEdge() {
        layOut("")
        placeCaret(at: 0)
        #expect(textView.isOnEdge(.up))
    }

    @Test func givenEmptyTextWhenMovingDownShouldBeOnEdge() {
        layOut("")
        placeCaret(at: 0)
        #expect(textView.isOnEdge(.down))
    }

    @Test func givenSingleLineWhenMovingUpShouldBeOnEdge() {
        layOut("hello")
        placeCaret(at: 2)
        #expect(textView.isOnEdge(.up))
    }

    @Test func givenSingleLineWhenMovingDownShouldBeOnEdge() {
        layOut("hello")
        placeCaret(at: 2)
        #expect(textView.isOnEdge(.down))
    }

    @Test func givenCaretOnFirstLineWhenMovingUpShouldBeOnEdge() {
        layOut(Self.hardLines)
        placeCaret(at: 1)
        #expect(textView.isOnEdge(.up))
    }

    @Test func givenCaretOnFirstLineWhenMovingDownShouldNotBeOnEdge() {
        layOut(Self.hardLines)
        placeCaret(at: 1)
        #expect(!textView.isOnEdge(.down))
    }

    @Test func givenCaretOnMiddleLineWhenMovingUpShouldNotBeOnEdge() {
        layOut(Self.hardLines)
        placeCaret(at: 5)
        #expect(!textView.isOnEdge(.up))
    }

    @Test func givenCaretOnMiddleLineWhenMovingDownShouldNotBeOnEdge() {
        layOut(Self.hardLines)
        placeCaret(at: 5)
        #expect(!textView.isOnEdge(.down))
    }

    @Test func givenCaretOnLastLineWhenMovingUpShouldNotBeOnEdge() {
        layOut(Self.hardLines)
        placeCaret(at: 10)
        #expect(!textView.isOnEdge(.up))
    }

    @Test func givenCaretOnLastLineWhenMovingDownShouldBeOnEdge() {
        layOut(Self.hardLines)
        placeCaret(at: 10)
        #expect(textView.isOnEdge(.down))
    }

    @Test func givenCaretAtWrapPointWithUpstreamAffinityWhenMovingUpShouldBeOnEdge() throws {
        layOut(Self.wrappedLines)
        try placeCaret(at: wrapPoint(), affinity: .upstream)
        #expect(textView.isOnEdge(.up))
    }

    @Test func givenCaretAtWrapPointWithUpstreamAffinityWhenMovingDownShouldNotBeOnEdge() throws {
        layOut(Self.wrappedLines)
        try placeCaret(at: wrapPoint(), affinity: .upstream)
        #expect(!textView.isOnEdge(.down))
    }

    @Test func givenCaretAtWrapPointWithDownstreamAffinityWhenMovingUpShouldNotBeOnEdge() throws {
        layOut(Self.wrappedLines)
        try placeCaret(at: wrapPoint(), affinity: .downstream)
        #expect(!textView.isOnEdge(.up))
    }

    @Test func givenCaretAtWrapPointWithDownstreamAffinityWhenMovingDownShouldBeOnEdge() throws {
        layOut(Self.wrappedLines)
        try placeCaret(at: wrapPoint(), affinity: .downstream)
        #expect(textView.isOnEdge(.down))
    }

    @Test func givenCaretAtEndOfLongerWrappedLineWhenMovingUpShouldNotBeOnEdge() throws {
        try layOutShortLineAboveLongerWrappedLine()
        placeCaret(at: (textView.string as NSString).length)
        #expect(!textView.isOnEdge(.up))
    }

    // MARK: - caretY

    @Test func givenCaretOnSecondLineWhenMeasuringShouldBeOneLineBelowFirstLine() throws {
        layOut(Self.hardLines)
        placeCaret(at: 0)
        let first = try #require(caretY())
        placeCaret(at: 5)
        let second = try #require(caretY())
        #expect(second - first == MultilineTextView.lineHeight)
    }

    @Test func givenCaretAtWrapPointWithUpstreamAffinityWhenMeasuringShouldReturnFirstLine() throws {
        layOut(Self.wrappedLines)
        placeCaret(at: 0)
        let firstLine = try #require(caretY())
        try placeCaret(at: wrapPoint(), affinity: .upstream)
        #expect(caretY() == firstLine)
    }

    @Test func givenCaretAtWrapPointWithDownstreamAffinityWhenMeasuringShouldReturnSecondLine() throws {
        layOut(Self.wrappedLines)
        placeCaret(at: 0)
        let firstLine = try #require(caretY())
        try placeCaret(at: wrapPoint(), affinity: .downstream)
        #expect(caretY() == firstLine + MultilineTextView.lineHeight)
    }

    // MARK: - doCommand

    @Test func givenInsertNewlineCommandWhenShiftIsNotHeldShouldCallOnSubmit() {
        layOut("hello")
        placeCaret(at: 5)
        var submitted = false
        textView.onSubmit = { submitted = true }
        textView.doCommand(by: #selector(NSResponder.insertNewline(_:)))
        #expect(submitted)
    }

    @Test func givenInsertNewlineCommandWhenShiftIsNotHeldShouldNotInsertNewline() {
        layOut("hello")
        placeCaret(at: 5)
        textView.doCommand(by: #selector(NSResponder.insertNewline(_:)))
        #expect(textView.string == "hello")
    }

    @Test func givenInsertNewlineIgnoringFieldEditorCommandWhenPerformedShouldInsertNewline() {
        layOut("hello")
        placeCaret(at: 5)
        textView.doCommand(by: #selector(NSResponder.insertNewlineIgnoringFieldEditor(_:)))
        #expect(textView.string == "hello\n")
    }

    @Test func givenCaretOnFirstLineWhenMoveUpCommandShouldCallOnNavigateUp() {
        layOut(Self.hardLines)
        placeCaret(at: 1)
        var delta: Int?
        textView.onNavigate = { delta = $0 }
        textView.doCommand(by: #selector(NSResponder.moveUp(_:)))
        #expect(delta == -1)
    }

    @Test func givenCaretOnFirstLineWhenMoveUpCommandShouldKeepCaret() {
        layOut(Self.hardLines)
        placeCaret(at: 1)
        textView.doCommand(by: #selector(NSResponder.moveUp(_:)))
        #expect(textView.selectedRange().location == 1)
    }

    @Test func givenCaretOnLastLineWhenMoveDownCommandShouldCallOnNavigateDown() {
        layOut(Self.hardLines)
        placeCaret(at: 10)
        var delta: Int?
        textView.onNavigate = { delta = $0 }
        textView.doCommand(by: #selector(NSResponder.moveDown(_:)))
        #expect(delta == 1)
    }

    @Test func givenCaretOnMiddleLineWhenMoveUpCommandShouldNotCallOnNavigate() {
        layOut(Self.hardLines)
        placeCaret(at: 5)
        var navigated = false
        textView.onNavigate = { _ in navigated = true }
        textView.doCommand(by: #selector(NSResponder.moveUp(_:)))
        #expect(!navigated)
    }

    @Test func givenCaretOnMiddleLineWhenMoveUpCommandShouldMoveCaretToFirstLine() {
        layOut(Self.hardLines)
        placeCaret(at: 5)
        textView.doCommand(by: #selector(NSResponder.moveUp(_:)))
        #expect(textView.selectedRange().location < 4)
    }

    @Test func givenCaretOnMiddleLineWhenMoveDownCommandShouldMoveCaretToLastLine() {
        layOut(Self.hardLines)
        placeCaret(at: 5)
        textView.doCommand(by: #selector(NSResponder.moveDown(_:)))
        #expect(textView.selectedRange().location > 7)
    }

    @Test func givenCancelOperationCommandWhenPerformedShouldCallOnEscape() {
        layOut("hello")
        var escaped = false
        textView.onEscape = { escaped = true }
        textView.doCommand(by: #selector(NSResponder.cancelOperation(_:)))
        #expect(escaped)
    }

    @Test func givenOtherCommandWhenPerformedShouldNotCallAnyCallback() {
        layOut("hello")
        placeCaret(at: 0)
        var called = false
        textView.onSubmit = { called = true }
        textView.onNavigate = { _ in called = true }
        textView.onEscape = { called = true }
        textView.doCommand(by: #selector(NSResponder.moveRight(_:)))
        #expect(!called)
    }

    @Test func givenOtherCommandWhenPerformedShouldApplyDefaultEditing() {
        layOut("hello")
        placeCaret(at: 0)
        textView.doCommand(by: #selector(NSResponder.moveRight(_:)))
        #expect(textView.selectedRange().location == 1)
    }

    @Test func givenInsertTabCommandWhenPerformedShouldNotInsertTab() {
        layOut("ab")
        placeCaret(at: 2)
        textView.doCommand(by: #selector(NSResponder.insertTab(_:)))
        #expect(textView.string == "ab")
    }

    @Test func givenInsertTabIgnoringFieldEditorCommandWhenPerformedShouldInsertTab() {
        layOut("ab")
        placeCaret(at: 2)
        textView.doCommand(by: #selector(NSResponder.insertTabIgnoringFieldEditor(_:)))
        #expect(textView.string == "ab\t")
    }

    // MARK: - contentHeight

    @Test func givenEmptyTextWhenMeasuringContentHeightShouldBeOneLine() {
        textView.string = ""
        #expect(textView.contentHeight == MultilineTextView.lineHeight)
    }

    @Test func givenSingleLineWhenMeasuringContentHeightShouldBeOneLine() {
        textView.string = "hello"
        #expect(textView.contentHeight == MultilineTextView.lineHeight)
    }

    @Test func givenThreeLinesWhenMeasuringContentHeightShouldBeThreeLines() {
        textView.string = Self.hardLines
        #expect(textView.contentHeight == MultilineTextView.lineHeight * 3)
    }

    @Test func givenTrailingNewlineWhenMeasuringContentHeightShouldCountTheEmptyLastLine() {
        textView.string = Self.hardLines + "\n"
        #expect(textView.contentHeight == MultilineTextView.lineHeight * 4)
    }

    @Test func givenSoftWrappedTextWhenMeasuringContentHeightShouldCountWrappedLines() throws {
        textView.string = Self.wrappedLines
        let height = textView.contentHeight
        try #require(lineStarts().count == 2)
        #expect(height == MultilineTextView.lineHeight * 2)
    }

    // MARK: - Marked text

    @Test func givenNoCompositionWhenSettingMarkedTextShouldHaveMarkedText() {
        layOut("")
        textView.setMarkedText("ni", selectedRange: NSRange(location: 2, length: 0), replacementRange: Self.noReplacement)
        #expect(textView.hasMarkedText())
    }

    @Test func givenNoCompositionWhenSettingMarkedTextShouldCallOnMarkedTextChange() {
        layOut("")
        var changed = false
        textView.onMarkedTextChange = { changed = true }
        textView.setMarkedText("ni", selectedRange: NSRange(location: 2, length: 0), replacementRange: Self.noReplacement)
        #expect(changed)
    }

    @Test func givenCompositionWhenSettingEmptyMarkedTextShouldHaveNoMarkedText() {
        layOut("")
        compose("ni")
        textView.setMarkedText("", selectedRange: NSRange(location: 0, length: 0), replacementRange: Self.noReplacement)
        #expect(!textView.hasMarkedText())
    }

    @Test func givenCompositionWhenSettingEmptyMarkedTextShouldCallOnMarkedTextChange() {
        layOut("")
        compose("ni")
        var changed = false
        textView.onMarkedTextChange = { changed = true }
        textView.setMarkedText("", selectedRange: NSRange(location: 0, length: 0), replacementRange: Self.noReplacement)
        #expect(changed)
    }

    @Test func givenCompositionWhenUnmarkingTextShouldHaveNoMarkedText() {
        layOut("")
        compose("ni")
        textView.unmarkText()
        #expect(!textView.hasMarkedText())
    }

    @Test func givenCompositionWhenUnmarkingTextShouldCallOnMarkedTextChange() {
        layOut("")
        compose("ni")
        var changed = false
        textView.onMarkedTextChange = { changed = true }
        textView.unmarkText()
        #expect(changed)
    }

    @Test func givenCompositionWhenInsertingTextShouldHaveNoMarkedText() {
        layOut("")
        compose("ni")
        textView.insertText("你", replacementRange: Self.noReplacement)
        #expect(!textView.hasMarkedText())
    }

    @Test func givenCompositionWhenInsertingTextShouldCallOnMarkedTextChange() {
        layOut("")
        compose("ni")
        var changed = false
        textView.onMarkedTextChange = { changed = true }
        textView.insertText("你", replacementRange: Self.noReplacement)
        #expect(changed)
    }

    // MARK: - Undo

    @Test func givenTypedTextWhenUndoingShouldRemoveTypedText() throws {
        try typeUndoably("hello")

        try undo()

        #expect(textView.string.isEmpty)
    }

    // MARK: - Clear

    @Test func givenTypedTextWhenClearedShouldBeEmpty() {
        layOut("hello")
        textView.clear()
        #expect(textView.string.isEmpty)
    }

    @Test func givenImageAndTextWhenClearedShouldRemoveBoth() throws {
        layOut("")
        try pasteImage()
        textView.insertText("hello", replacementRange: textView.selectedRange())
        textView.clear()
        #expect(textView.string.isEmpty)
        #expect(attachments().isEmpty)
    }

    @Test func givenEmptyTextWhenClearedShouldStayEmpty() {
        layOut("")
        textView.clear()
        #expect(textView.string.isEmpty)
    }

    @Test func givenTypedTextWhenClearedShouldPostTextDidChange() {
        let recorder = TextChangeRecorder()
        textView.delegate = recorder
        layOut("hello")
        textView.clear()
        #expect(recorder.changes == 1)
    }

    @Test func givenTypedTextWhenClearedThenUndoingShouldRestoreTypedText() throws {
        try typeUndoably("hello")

        try clearUndoably()
        try #require(textView.string.isEmpty)
        try undo()

        #expect(textView.string == "hello")
    }

    @Test func givenTypedTextWhenClearedThenUndoingTwiceShouldRemoveTypedText() throws {
        try typeUndoably("hello")
        try clearUndoably()

        try undo()
        try #require(textView.string == "hello")
        try undo()

        #expect(textView.string.isEmpty)
    }

    // MARK: - Helpers

    private func layOutShortLineAboveLongerWrappedLine() throws {
        for length in 10 ... 80 {
            layOut("ab " + String(repeating: "y", count: length))
            if lineStarts().count == 2 {
                return
            }
        }
        Issue.record("no text length produced a short line above a longer wrapped line")
    }

    private func typeUndoably(_ text: String) throws {
        let undoManager = try #require(textView.undoManager)
        undoManager.groupsByEvent = false
        undoManager.beginUndoGrouping()
        textView.insertText(text, replacementRange: textView.selectedRange())
        undoManager.endUndoGrouping()
    }

    private func clearUndoably() throws {
        let undoManager = try #require(textView.undoManager)
        undoManager.groupsByEvent = false
        undoManager.beginUndoGrouping()
        textView.clear()
        undoManager.endUndoGrouping()
    }

    private func undo() throws {
        let undoManager = try #require(textView.undoManager)
        try #require(undoManager.canUndo)
        undoManager.undo()
    }
}
