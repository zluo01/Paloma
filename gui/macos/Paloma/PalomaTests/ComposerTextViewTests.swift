//
//  ComposerTextViewTests.swift
//  PalomaTests
//

import AppKit
@testable import Paloma
import Testing
import UniformTypeIdentifiers

@MainActor
@Suite(.serialized)
struct ComposerTextViewTests {
    private static let hardLines = "one\ntwo\nthree"
    private static let wrappedLines = String(repeating: "wrap words ", count: 5)

    private let window: NSWindow
    private let textView: ComposerTextView

    init() {
        textView = ComposerTextView.make()
        textView.frame = NSRect(x: 0, y: 0, width: 300, height: 200)
        window = NSWindow(contentRect: textView.frame, styleMask: [.borderless], backing: .buffered, defer: false)
        window.contentView = textView
    }

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
        #expect(second - first == ComposerTextView.lineHeight)
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
        #expect(caretY() == firstLine + ComposerTextView.lineHeight)
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
        #expect(textView.contentHeight == ComposerTextView.lineHeight)
    }

    @Test func givenSingleLineWhenMeasuringContentHeightShouldBeOneLine() {
        textView.string = "hello"
        #expect(textView.contentHeight == ComposerTextView.lineHeight)
    }

    @Test func givenThreeLinesWhenMeasuringContentHeightShouldBeThreeLines() {
        textView.string = Self.hardLines
        #expect(textView.contentHeight == ComposerTextView.lineHeight * 3)
    }

    @Test func givenTrailingNewlineWhenMeasuringContentHeightShouldCountTheEmptyLastLine() {
        textView.string = Self.hardLines + "\n"
        #expect(textView.contentHeight == ComposerTextView.lineHeight * 4)
    }

    @Test func givenSoftWrappedTextWhenMeasuringContentHeightShouldCountWrappedLines() throws {
        textView.string = Self.wrappedLines
        let height = textView.contentHeight
        try #require(lineStarts().count == 2)
        #expect(height == ComposerTextView.lineHeight * 2)
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

    // MARK: - Pasting files

    @Test func givenFilesOnPasteboardWhenChoosingTypeShouldPreferFileURL() {
        let board = pasteboard(files: ["/Users/a/a.jpg"])
        #expect(textView.preferredPasteboardType(from: board.types ?? [], restrictedToTypesFrom: nil) == .fileURL)
    }

    @Test func givenTextAndImageOnPasteboardWhenChoosingTypeShouldPreferText() throws {
        let board = try pasteboard {
            $0.declareTypes([.string, .png], owner: nil)
            $0.setString("A1\tB1", forType: .string)
            try $0.setData(imageData(.png), forType: .png)
        }
        defer { board.releaseGlobally() }
        let chosen = textView.preferredPasteboardType(from: board.types ?? [], restrictedToTypesFrom: nil)
        #expect(chosen != nil && chosen != .png)
    }

    @Test func givenTextAndImageOnPasteboardWhenPastingShouldInsertTheText() throws {
        layOut("")
        try paste(pasteboard {
            $0.declareTypes([.string, .png], owner: nil)
            $0.setString("A1\tB1", forType: .string)
            try $0.setData(imageData(.png), forType: .png)
        })
        #expect(textView.string == "A1\tB1")
        #expect(attachments().isEmpty)
    }

    @Test func givenSingleFileOnPasteboardWhenPastingShouldInsertQuotedPath() {
        layOut("")
        paste(pasteboard(files: ["/Users/a/a.jpg"]))
        #expect(textView.string == "'/Users/a/a.jpg' ")
    }

    @Test func givenTwoFilesOnPasteboardWhenPastingShouldInsertQuotedPathsJoinedBySpace() {
        layOut("")
        paste(pasteboard(files: ["/Users/a/a.jpg", "/Users/a/b.jpg"]))
        #expect(textView.string == "'/Users/a/a.jpg' '/Users/a/b.jpg' ")
    }

    @Test func givenFilePathWithSpacesWhenPastingShouldKeepSpacesInsideQuotes() {
        layOut("")
        paste(pasteboard(files: ["/Users/a/My Notes/plan b.txt"]))
        #expect(textView.string == "'/Users/a/My Notes/plan b.txt' ")
    }

    @Test func givenFilePathWithSingleQuoteWhenPastingShouldEscapeTheQuote() {
        layOut("")
        paste(pasteboard(files: ["/Users/a/it's.txt"]))
        #expect(textView.string == #"'/Users/a/it'\''s.txt' "#)
    }

    @Test func givenCaretInsideTextWhenPastingFilesShouldInsertAtCaret() {
        layOut("see now")
        placeCaret(at: 4)
        paste(pasteboard(files: ["/Users/a/a.jpg"]))
        #expect(textView.string == "see '/Users/a/a.jpg' now")
    }

    @Test func givenSelectedTextWhenPastingFilesShouldReplaceSelection() {
        layOut("see this now")
        textView.setSelectedRange(NSRange(location: 4, length: 5))
        paste(pasteboard(files: ["/Users/a/a.jpg"]))
        #expect(textView.string == "see '/Users/a/a.jpg' now")
    }

    @Test func givenPlainTextOnPasteboardWhenPastingShouldInsertTextUnchanged() {
        layOut("")
        paste(pasteboard { $0.setString("hello world", forType: .string) })
        #expect(textView.string == "hello world")
    }

    @Test func givenWebLinkOnPasteboardWhenPastingShouldInsertLinkUnchanged() {
        layOut("")
        paste(pasteboard {
            $0.writeObjects([URL(string: "https://example.com/a%20b")! as NSURL])
            $0.setString("https://example.com/a%20b", forType: .string)
        })
        #expect(textView.string == "https://example.com/a%20b")
    }

    // MARK: - Pasting images

    @Test(arguments: [UTType.png, .jpeg, .gif])
    func givenImageFileWhenPastingShouldInsertImageWithOriginalData(type: UTType) throws {
        layOut("")
        let data = try imageData(type)
        try withTemporaryFile(type, data) { path in
            paste(pasteboard(files: [path]))
        }
        #expect(textView.string == "\u{FFFC} ")
        #expect(attachments().map(\.type) == [type])
        #expect(attachments().map(\.data) == [data])
    }

    @Test(arguments: [UTType.tiff, .pdf])
    func givenFileThatIsNotAnAttachableImageWhenPastingShouldInsertQuotedPath(type: UTType) throws {
        layOut("")
        try withTemporaryFile(type, imageData(.tiff)) { path in
            paste(pasteboard(files: [path]))
            #expect(textView.string == "'\(path)' ")
        }
        #expect(attachments().isEmpty)
    }

    @Test func givenImageAndTextFilesWhenPastingShouldInsertImageAndQuotedPathJoinedBySpace() throws {
        layOut("")
        try withTemporaryFile(.png, imageData(.png)) { image in
            try withTemporaryFile(.plainText, Data("notes".utf8)) { text in
                paste(pasteboard(files: [image, text]))
                #expect(textView.string == "\u{FFFC} '\(text)' ")
            }
        }
        #expect(attachments().map(\.type) == [.png])
    }

    @Test func givenPNGDataOnPasteboardWhenPastingShouldInsertImageWithOriginalData() throws {
        layOut("")
        let data = try imageData(.png)
        paste(pasteboard { $0.setData(data, forType: .png) })
        #expect(attachments().map(\.type) == [.png])
        #expect(attachments().map(\.data) == [data])
    }

    @Test func givenOnlyTIFFDataOnPasteboardWhenPastingShouldInsertImageConvertedToPNG() throws {
        layOut("")
        try paste(pasteboard { try $0.setData(imageData(.tiff), forType: .tiff) })
        #expect(attachments().map(\.type) == [.png])
        #expect(attachments().first?.data.starts(with: [0x89, 0x50, 0x4E, 0x47]) == true)
    }

    @Test func givenOnlyPDFDataOnPasteboardWhenPastingShouldNotInsertImage() {
        layOut("")
        paste(pasteboard { $0.setData(Data("%PDF-1.4".utf8), forType: .pdf) })
        #expect(attachments().isEmpty)
    }

    @Test func givenOversizedImageDataOnPasteboardWhenPastingShouldNotInsertImage() {
        layOut("")
        paste(pasteboard { $0.setData(Data(count: ComposerTextView.maxImageBytes + 1), forType: .png) })
        #expect(attachments().isEmpty)
        #expect(textView.string.isEmpty)
    }

    @Test func givenOversizedImageFileWhenPastingShouldInsertQuotedPath() throws {
        layOut("")
        try withTemporaryFile(.png, Data(count: ComposerTextView.maxImageBytes + 1)) { path in
            paste(pasteboard(files: [path]))
            #expect(textView.string == "'\(path)' ")
        }
        #expect(attachments().isEmpty)
    }

    @Test func givenImageDataOnPasteboardWhenCheckingReadableTypesShouldAcceptPNGAndTIFF() {
        #expect(textView.readablePasteboardTypes.contains(.png))
        #expect(textView.readablePasteboardTypes.contains(.tiff))
    }

    @Test func givenLargeImageWhenPastingShouldScaleDownToOneLineKeepingAspectRatio() throws {
        layOut("")
        try paste(pasteboard { try $0.setData(imageData(.png, width: 400, height: 200), forType: .png) })
        let bounds = try #require(attachments().first?.bounds)
        #expect(bounds.height == ComposerTextView.lineHeight)
        #expect(bounds.width == ComposerTextView.lineHeight * 2)
    }

    @Test func givenSmallImageWhenPastingShouldKeepItsSize() throws {
        layOut("")
        try paste(pasteboard { try $0.setData(imageData(.png, width: 20, height: 10), forType: .png) })
        let bounds = try #require(attachments().first?.bounds)
        #expect(bounds.size == CGSize(width: 20, height: 10))
    }

    // MARK: - Image preview

    @Test func givenImageWhenFindingImageAtItsCenterShouldReturnIt() throws {
        layOut("")
        try paste(pasteboard { try $0.setData(imageData(.png, width: 400, height: 200), forType: .png) })
        let frame = try #require(textView.imageAttachment(at: CGPoint(x: 5, y: 10))?.frame)
        #expect(textView.imageAttachment(at: CGPoint(x: frame.midX, y: frame.midY)) != nil)
    }

    @Test func givenTextBesideImageWhenFindingImageOverTheTextShouldReturnNil() throws {
        layOut("")
        try paste(pasteboard { try $0.setData(imageData(.png, width: 400, height: 200), forType: .png) })
        textView.insertText(" some words after", replacementRange: textView.selectedRange())
        let frame = try #require(textView.imageAttachment(at: CGPoint(x: 5, y: 10))?.frame)
        #expect(textView.imageAttachment(at: CGPoint(x: frame.maxX + 40, y: frame.midY)) == nil)
    }

    @Test func givenSmallImageWhenFindingImageAboveItInTheSameLineShouldReturnNil() throws {
        layOut("")
        try paste(pasteboard { try $0.setData(imageData(.png, width: 20, height: 10), forType: .png) })
        let frame = try #require(textView.imageAttachment(at: CGPoint(x: 5, y: ComposerTextView.lineHeight - 2))?.frame)
        #expect(textView.imageAttachment(at: CGPoint(x: frame.midX, y: frame.minY - 3)) == nil)
    }

    @Test func givenPointerMovedOverImageWhenPreviewDelayPassesShouldShowPreview() async throws {
        let panel = try await keyPanel()
        defer { panel.close() }
        try paste(pasteboard { try $0.setData(imageData(.png, width: 400, height: 200), forType: .png) })
        let frame = try #require(textView.imageAttachment(at: CGPoint(x: 5, y: 10))?.frame)

        try movePointer(to: CGPoint(x: frame.midX, y: frame.midY), in: panel)
        try await Task.sleep(for: .milliseconds(500))

        #expect(textView.preview.isShown)
        textView.preview.close()
    }

    @Test func givenPreviewShownWhenPointerMovesOffImageShouldClosePreview() async throws {
        let panel = try await keyPanel()
        defer { panel.close() }
        try paste(pasteboard { try $0.setData(imageData(.png, width: 400, height: 200), forType: .png) })
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
        let panel = try await keyPanel()
        defer { panel.close() }
        try paste(pasteboard { try $0.setData(imageData(.png, width: 1200, height: 400), forType: .png) })
        let frame = try #require(textView.imageAttachment(at: CGPoint(x: 5, y: 10))?.frame)

        try movePointer(to: CGPoint(x: frame.midX, y: frame.midY), in: panel)
        try await Task.sleep(for: .milliseconds(500))

        #expect(textView.preview.contentSize == CGSize(width: 360, height: 120))
        textView.preview.close()
    }

    // MARK: - Dropping

    @Test func givenOtherWindowIsKeyWhenDroppingImageShouldMakeComposerKeyAgain() async throws {
        let panel = NSPanel(contentRect: textView.frame, styleMask: [.titled, .nonactivatingPanel], backing: .buffered, defer: false)
        panel.contentView = textView
        panel.orderFront(nil)
        let other = NSPanel(contentRect: textView.frame, styleMask: [.titled, .nonactivatingPanel], backing: .buffered, defer: false)
        other.makeKeyAndOrderFront(nil)
        defer {
            panel.close()
            other.close()
        }
        for _ in 0 ..< 20 where !other.isKeyWindow {
            try await Task.sleep(for: .milliseconds(100))
        }
        try #require(other.isKeyWindow)

        let board = try pasteboard { try $0.setData(imageData(.png), forType: .png) }
        defer { board.releaseGlobally() }
        let drop = DropInfo(board: board, location: textView.convert(CGPoint(x: 5, y: 10), to: nil), window: panel)
        _ = textView.draggingEntered(drop)
        let accepted = textView.performDragOperation(drop)
        textView.concludeDragOperation(drop)
        for _ in 0 ..< 10 where !panel.isKeyWindow {
            try await Task.sleep(for: .milliseconds(100))
        }

        #expect(accepted)
        #expect(attachments().map(\.type) == [.png])
        #expect(panel.isKeyWindow)
        #expect(panel.firstResponder === textView)
    }

    // MARK: - Undo

    @Test func givenTypedTextWhenUndoingShouldRemoveTypedText() async throws {
        let panel = try await keyPanel()
        defer { panel.close() }
        try typeUndoably("hello")

        try await pressCommandZ(in: panel)

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
        try paste(pasteboard { try $0.setData(imageData(.png), forType: .png) })
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

    @Test func givenTypedTextWhenClearedThenUndoingShouldRestoreTypedText() async throws {
        let panel = try await keyPanel()
        defer { panel.close() }
        try typeUndoably("hello")

        try clearUndoably()
        try #require(textView.string.isEmpty)
        try await pressCommandZ(in: panel)

        #expect(textView.string == "hello")
    }

    @Test func givenTypedTextWhenClearedThenUndoingTwiceShouldRemoveTypedText() async throws {
        let panel = try await keyPanel()
        defer { panel.close() }
        try typeUndoably("hello")
        try clearUndoably()

        try await pressCommandZ(in: panel)
        try #require(textView.string == "hello")
        try await pressCommandZ(in: panel)

        #expect(textView.string.isEmpty)
    }

    // MARK: - Helpers

    private func keyPanel() async throws -> NSPanel {
        let panel = NSPanel(contentRect: textView.frame, styleMask: [.titled, .nonactivatingPanel], backing: .buffered, defer: false)
        panel.contentView = textView
        panel.makeKeyAndOrderFront(nil)
        panel.makeFirstResponder(textView)
        for _ in 0 ..< 20 where !panel.isKeyWindow {
            try await Task.sleep(for: .milliseconds(100))
        }
        try #require(panel.isKeyWindow)
        return panel
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

    private func pressCommandZ(in panel: NSPanel) async throws {
        let before = textView.string
        let commandZ = try #require(NSEvent.keyEvent(
            with: .keyDown,
            location: .zero,
            modifierFlags: .command,
            timestamp: 0,
            windowNumber: panel.windowNumber,
            context: nil,
            characters: "z",
            charactersIgnoringModifiers: "z",
            isARepeat: false,
            keyCode: 6
        ))
        NSApp.postEvent(commandZ, atStart: false)
        for _ in 0 ..< 10 where textView.string == before {
            try await Task.sleep(for: .milliseconds(100))
        }
    }

    private static let noReplacement = NSRange(location: NSNotFound, length: 0)

    private func pasteboard(_ fill: (NSPasteboard) throws -> Void) rethrows -> NSPasteboard {
        let board = NSPasteboard.withUniqueName()
        board.clearContents()
        try fill(board)
        return board
    }

    private func pasteboard(files paths: [String]) -> NSPasteboard {
        pasteboard { $0.writeObjects(paths.map { URL(fileURLWithPath: $0) as NSURL }) }
    }

    private func withTemporaryFile(_ type: UTType, _ data: Data, _ body: (String) throws -> Void) throws {
        let url = URL.temporaryDirectory.appendingPathComponent("ComposerTextViewTests-\(UUID())", conformingTo: type)
        try data.write(to: url)
        defer { try? FileManager.default.removeItem(at: url) }
        try body(url.path(percentEncoded: false))
    }

    private struct PastedImage {
        let type: UTType?
        let data: Data
        let bounds: CGRect
    }

    private func attachments() -> [PastedImage] {
        guard let storage = textView.textStorage else { return [] }
        var found: [PastedImage] = []
        storage.enumerateAttribute(.attachment, in: NSRange(location: 0, length: storage.length)) { value, _, _ in
            guard let attachment = value as? NSTextAttachment else { return }
            found.append(PastedImage(
                type: attachment.fileType.flatMap(UTType.init),
                data: attachment.contents ?? Data(),
                bounds: attachment.bounds
            ))
        }
        return found
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

    private func paste(_ board: NSPasteboard) {
        defer { board.releaseGlobally() }
        _ = textView.readSelection(from: board)
    }

    private func compose(_ text: String) {
        textView.setMarkedText(text, selectedRange: NSRange(location: (text as NSString).length, length: 0), replacementRange: Self.noReplacement)
    }

    private func layOut(_ text: String) {
        textView.string = text
        guard let layout = textView.textLayoutManager else { return }
        layout.ensureLayout(for: layout.documentRange)
    }

    private func layOutShortLineAboveLongerWrappedLine() throws {
        for length in 10 ... 80 {
            layOut("ab " + String(repeating: "y", count: length))
            if lineStarts().count == 2 {
                return
            }
        }
        Issue.record("no text length produced a short line above a longer wrapped line")
    }

    private func placeCaret(at offset: Int, affinity: NSSelectionAffinity = .downstream) {
        textView.setSelectedRange(NSRange(location: offset, length: 0), affinity: affinity, stillSelecting: false)
    }

    private func caretY() -> CGFloat? {
        guard let layout = textView.textLayoutManager, let selection = layout.textSelections.first else { return nil }
        return textView.caretY(of: selection, in: layout)
    }

    private func wrapPoint() throws -> Int {
        let starts = lineStarts()
        try #require(starts.count == 2, "the fixture text should wrap onto exactly two lines")
        return starts[1]
    }

    private func lineStarts() -> [Int] {
        guard let layout = textView.textLayoutManager, let content = layout.textContentManager else { return [] }
        var starts: [Int] = []
        layout.enumerateTextLayoutFragments(from: layout.documentRange.location, options: [.ensuresLayout]) { fragment in
            let fragmentStart = content.offset(from: content.documentRange.location, to: fragment.rangeInElement.location)
            for line in fragment.textLineFragments {
                starts.append(fragmentStart + line.characterRange.location)
            }
            return true
        }
        return starts
    }
}

@MainActor
private final class TextChangeRecorder: NSObject, NSTextViewDelegate {
    private(set) var changes = 0

    func textDidChange(_: Notification) {
        changes += 1
    }
}

private final class DropInfo: NSObject, NSDraggingInfo {
    let draggingPasteboard: NSPasteboard
    let draggingLocation: NSPoint
    private weak var window: NSWindow?

    init(board: NSPasteboard, location: NSPoint, window: NSWindow) {
        draggingPasteboard = board
        draggingLocation = location
        self.window = window
    }

    var draggingDestinationWindow: NSWindow? {
        window
    }

    var draggingSourceOperationMask: NSDragOperation {
        .copy
    }

    var draggedImageLocation: NSPoint {
        draggingLocation
    }

    var draggedImage: NSImage? {
        nil
    }

    var draggingSource: Any? {
        nil
    }

    var draggingSequenceNumber: Int {
        1
    }

    var draggingFormation: NSDraggingFormation = .default
    var animatesToDestination = false
    var numberOfValidItemsForDrop = 1

    var springLoadingHighlight: NSSpringLoadingHighlight {
        .none
    }

    func slideDraggedImage(to _: NSPoint) {}

    func enumerateDraggingItems(
        options _: NSDraggingItemEnumerationOptions,
        for _: NSView?,
        classes _: [AnyClass],
        searchOptions _: [NSPasteboard.ReadingOptionKey: Any],
        using _: (NSDraggingItem, Int, UnsafeMutablePointer<ObjCBool>) -> Void
    ) {}

    func resetSpringLoading() {}
}
