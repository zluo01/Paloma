//
//  QueryModelTests.swift
//  PalomaTests
//

import AppKit
@testable import Paloma
import Testing

@MainActor
struct QueryModelTests {
    private let textView = ComposerTextView.make()
    private let query = QueryModel()

    init() {
        query.textView = textView
    }

    @Test func givenNewModelWhenCreatedShouldBeEmpty() {
        #expect(QueryModel().isEmpty)
        #expect(QueryModel().text.isEmpty)
    }

    @Test func givenTypedTextWhenSyncedShouldExposeText() {
        textView.insertText("hello", replacementRange: textView.selectedRange())
        query.sync()
        #expect(query.text == "hello")
        #expect(!query.isEmpty)
    }

    @Test func givenImageAndTextWhenSyncedShouldExposeTextWithoutTheImage() throws {
        try pasteImage()
        textView.insertText("hi", replacementRange: textView.selectedRange())
        query.sync()
        #expect(query.text == "hi")
    }

    @Test func givenOnlyAnImageWhenSyncedShouldNotBeEmpty() throws {
        try pasteImage()
        query.sync()
        #expect(query.text.isEmpty)
        #expect(!query.isEmpty)
    }

    @Test func givenOnlyWhitespaceWhenSyncedShouldHaveNoPrompt() {
        textView.insertText("  \n ", replacementRange: textView.selectedRange())
        query.sync()
        #expect(!query.hasQuery)
    }

    @Test func givenTextWhenSyncedShouldHavePrompt() {
        textView.insertText(" hello ", replacementRange: textView.selectedRange())
        query.sync()
        #expect(query.hasQuery)
    }

    @Test func givenOnlyTextWhenBuildingPromptShouldReturnTrimmedTextWithoutAttachments() {
        textView.insertText("  hello  ", replacementRange: textView.selectedRange())
        query.sync()
        let prompt = query.prompt()
        #expect(prompt.text == "hello")
        #expect(prompt.attachments.isEmpty)
    }

    @Test func givenImageBetweenTextWhenBuildingPromptShouldReplaceItWithPlaceholder() throws {
        textView.insertText("look ", replacementRange: textView.selectedRange())
        let data = try pasteImage()
        textView.insertText(" here", replacementRange: textView.selectedRange())
        query.sync()
        let prompt = query.prompt()
        #expect(prompt.text == "look [Image #1] here")
        #expect(prompt.attachments == [.image(id: 1, mediaType: "image/png", data: data)])
    }

    @Test func givenTwoImagesWhenBuildingPromptShouldNumberThemInOrder() throws {
        let first = try pasteImage()
        textView.insertText(" and ", replacementRange: textView.selectedRange())
        let second = try pasteImage(.jpeg, as: NSPasteboard.PasteboardType("public.jpeg"))
        query.sync()
        let prompt = query.prompt()
        #expect(prompt.text == "[Image #1] and [Image #2]")
        #expect(prompt.attachments == [
            .image(id: 1, mediaType: "image/png", data: first),
            .image(id: 2, mediaType: "image/jpeg", data: second),
        ])
    }

    @Test func givenOnlyAnImageWhenBuildingPromptShouldReturnJustThePlaceholder() throws {
        try pasteImage()
        query.sync()
        #expect(query.prompt().text == "[Image #1]")
    }

    @Test func givenContentWhenClearedShouldEmptyTextViewAndModel() throws {
        try pasteImage()
        textView.insertText("hello", replacementRange: textView.selectedRange())
        query.sync()

        query.clear()

        #expect(textView.string.isEmpty)
        #expect(query.isEmpty)
        #expect(query.text.isEmpty)
    }

    @discardableResult
    private func pasteImage(_ type: NSBitmapImageRep.FileType = .png, as pasteboardType: NSPasteboard.PasteboardType = .png) throws -> Data {
        let image = NSImage(size: NSSize(width: 4, height: 2), flipped: false) { rect in
            NSColor.systemTeal.setFill()
            rect.fill()
            return true
        }
        let tiff = try #require(image.tiffRepresentation)
        let data = try #require(NSBitmapImageRep(data: tiff)?.representation(using: type, properties: [:]))
        let board = NSPasteboard.withUniqueName()
        defer { board.releaseGlobally() }
        board.clearContents()
        board.setData(data, forType: pasteboardType)
        try #require(textView.readSelection(from: board))
        return data
    }
}
