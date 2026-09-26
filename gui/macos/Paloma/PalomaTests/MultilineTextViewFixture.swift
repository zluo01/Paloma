//
//  MultilineTextViewFixture.swift
//  PalomaTests
//

import AppKit
@testable import Paloma
import Testing
import UniformTypeIdentifiers

@MainActor
class MultilineTextViewFixture {
    static let hardLines = "one\ntwo\nthree"
    static let wrappedLines = String(repeating: "wrap words ", count: 5)
    static let noReplacement = NSRange(location: NSNotFound, length: 0)

    let window: NSWindow
    let textView: MultilineTextView

    init() {
        textView = MultilineTextView.make()
        textView.frame = NSRect(x: 0, y: 0, width: 300, height: 200)
        window = NSWindow(contentRect: textView.frame, styleMask: [.borderless], backing: .buffered, defer: false)
        window.contentView = textView
    }

    struct PastedImage {
        let type: UTType?
        let data: Data
        let bounds: CGRect
    }

    func layOut(_ text: String) {
        textView.string = text
        guard let layout = textView.textLayoutManager else { return }
        layout.ensureLayout(for: layout.documentRange)
    }

    func placeCaret(at offset: Int, affinity: NSSelectionAffinity = .downstream) {
        textView.setSelectedRange(NSRange(location: offset, length: 0), affinity: affinity, stillSelecting: false)
    }

    func compose(_ text: String) {
        textView.setMarkedText(text, selectedRange: NSRange(location: (text as NSString).length, length: 0), replacementRange: Self.noReplacement)
    }

    func caretY() -> CGFloat? {
        guard let layout = textView.textLayoutManager, let selection = layout.textSelections.first else { return nil }
        return textView.caretY(of: selection, in: layout)
    }

    func wrapPoint() throws -> Int {
        let starts = lineStarts()
        try #require(starts.count == 2, "the fixture text should wrap onto exactly two lines")
        return starts[1]
    }

    func lineStarts() -> [Int] {
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

    func pasteboard(_ fill: (NSPasteboard) throws -> Void) rethrows -> NSPasteboard {
        let board = NSPasteboard.withUniqueName()
        board.clearContents()
        try fill(board)
        return board
    }

    func pasteboard(files paths: [String]) -> NSPasteboard {
        pasteboard { $0.writeObjects(paths.map { URL(fileURLWithPath: $0) as NSURL }) }
    }

    func paste(_ board: NSPasteboard) {
        defer { board.releaseGlobally() }
        _ = textView.readSelection(from: board)
    }

    func pasteImage(width: Int = 4, height: Int = 2) throws {
        try paste(pasteboard { try $0.setData(imageData(.png, width: width, height: height), forType: .png) })
    }

    func attachments() -> [PastedImage] {
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

    func withTemporaryFile(_ type: UTType, _ data: Data, _ body: (String) throws -> Void) throws {
        let url = URL.temporaryDirectory.appendingPathComponent("MultilineTextViewTests-\(UUID())", conformingTo: type)
        try data.write(to: url)
        defer { try? FileManager.default.removeItem(at: url) }
        try body(url.path(percentEncoded: false))
    }
}
