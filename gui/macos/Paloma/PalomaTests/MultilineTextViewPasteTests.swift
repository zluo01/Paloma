//
//  MultilineTextViewPasteTests.swift
//  PalomaTests
//

import AppKit
@testable import Paloma
import Testing
import UniformTypeIdentifiers

@MainActor
@Suite(.serialized)
final class MultilineTextViewPasteTests: MultilineTextViewFixture {
    // MARK: - Choosing a type

    @Test func givenFilesOnPasteboardWhenChoosingTypeShouldPreferFileURL() {
        let board = pasteboard(files: ["/Users/a/a.jpg"])
        defer { board.releaseGlobally() }
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

    @Test func givenImageDataOnPasteboardWhenCheckingReadableTypesShouldAcceptPNGAndTIFF() {
        #expect(textView.readablePasteboardTypes.contains(.png))
        #expect(textView.readablePasteboardTypes.contains(.tiff))
    }

    // MARK: - Pasting text and files

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
        paste(pasteboard { $0.setData(Data(count: MultilineTextView.maxImageBytes + 1), forType: .png) })
        #expect(attachments().isEmpty)
        #expect(textView.string.isEmpty)
    }

    @Test func givenOversizedImageFileWhenPastingShouldInsertQuotedPath() throws {
        layOut("")
        try withTemporaryFile(.png, Data(count: MultilineTextView.maxImageBytes + 1)) { path in
            paste(pasteboard(files: [path]))
            #expect(textView.string == "'\(path)' ")
        }
        #expect(attachments().isEmpty)
    }

    // MARK: - Thumbnail size

    @Test func givenLargeImageWhenPastingShouldScaleDownToOneLineKeepingAspectRatio() throws {
        layOut("")
        try pasteImage(width: 400, height: 200)
        let bounds = try #require(attachments().first?.bounds)
        #expect(bounds.height == MultilineTextView.lineHeight)
        #expect(bounds.width == MultilineTextView.lineHeight * 2)
    }

    @Test func givenSmallImageWhenPastingShouldKeepItsSize() throws {
        layOut("")
        try pasteImage(width: 20, height: 10)
        let bounds = try #require(attachments().first?.bounds)
        #expect(bounds.size == CGSize(width: 20, height: 10))
    }

    @Test func givenWideImageWhenPastingShouldDisplayACroppedThumbnailNotAStretchedOne() throws {
        layOut("")
        try pasteImage(width: 400, height: 40)
        let storage = try #require(textView.textStorage)
        let attachment = try #require(storage.attribute(.attachment, at: 0, effectiveRange: nil) as? NSTextAttachment)
        let displayed = try #require(attachment.image(forBounds: attachment.bounds, textContainer: nil, characterIndex: 0))
        #expect(attachment.bounds.size == CGSize(width: MultilineTextView.lineHeight * 3, height: MultilineTextView.lineHeight))
        #expect(displayed.size == attachment.bounds.size)
        #expect(try #require(displayed.cgImage(forProposedRect: nil, context: nil, hints: nil)).width == 120)
    }

    @Test func givenWideImageWhenPastingShouldKeepTheOriginalBytesForSending() throws {
        layOut("")
        let data = try imageData(.png, width: 400, height: 40)
        paste(pasteboard { $0.setData(data, forType: .png) })
        #expect(attachments().map(\.data) == [data])
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
