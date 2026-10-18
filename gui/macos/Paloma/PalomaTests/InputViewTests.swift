//
//  InputViewTests.swift
//  PalomaTests
//

import AppKit
@testable import Paloma
import SwiftUI
import Testing

@MainActor
@Suite(.serialized)
struct InputViewTests {
    private static let panelWidth: CGFloat = 640
    private static let longText = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat."

    @Test func givenHostedInputWhenFirstLaidOutShouldSizeTextViewToVisibleWidth() throws {
        try withHostedInput { host, textView in
            #expect(abs(textView.frame.width - host.visibleWidth) < 0.5)
        }
    }

    @Test func givenLongTextWhenTypedShouldKeepTextViewAtVisibleWidthAfterEveryWord() throws {
        try withHostedInput { host, textView in
            let widths = host.type(Self.longText, into: textView).map(\.textViewWidth)
            #expect(widths.allSatisfy { abs($0 - host.visibleWidth) < 0.5 }, "\(widths)")
        }
    }

    @Test func givenLongTextWhenTypedShouldWrapOntoSeveralLinesAfterEveryWordThatOverflows() throws {
        try withHostedInput { host, textView in
            let overflowing = host.type(Self.longText, into: textView).filter { $0.usedWidth > host.visibleWidth || $0.lines > 1 }
            try #require(!overflowing.isEmpty, "the text never grew past one visible line")
            #expect(overflowing.allSatisfy { $0.lines > 1 }, "\(overflowing)")
        }
    }

    @Test func givenLongTextWhenTypedShouldLayOutWithinVisibleWidthAfterEveryWord() throws {
        try withHostedInput { host, textView in
            let used = host.type(Self.longText, into: textView).map(\.usedWidth)
            #expect(used.allSatisfy { $0 <= host.visibleWidth }, "\(used)")
        }
    }

    @Test func givenLongTextWhenTypedShouldGrowToTheWrappedHeight() throws {
        try withHostedInput { host, textView in
            let last = try #require(host.type(Self.longText, into: textView).last)
            let scrollView = try #require(textView.enclosingScrollView)
            let expected = CGFloat(min(last.lines, MultilineTextView.maxLines)) * MultilineTextView.lineHeight
            #expect(abs(scrollView.frame.height - expected) < 0.5, "\(last)")
        }
    }

    private func withHostedInput(_ body: (Host, MultilineTextView) throws -> Void) throws {
        let host = Host(width: Self.panelWidth)
        defer { host.tearDown() }
        try body(host, host.textView())
    }

    struct Measurement: CustomStringConvertible {
        let textViewWidth: CGFloat
        let usedWidth: CGFloat
        let lines: Int

        var description: String {
            "(textView: \(textViewWidth), used: \(usedWidth), lines: \(lines))"
        }
    }

    @MainActor
    final class Host {
        let query = QueryModel()
        let window: NSWindow
        let hosting: NSHostingView<AnyView>

        init(width: CGFloat) {
            let query = query
            let root = VStack(spacing: 0) {
                QueryView(query: query, mode: .search, onSearch: { _ in }, onSubmit: {}, onNavigate: { _ in }, onEscape: {})
            }
            .frame(width: width)
            hosting = NSHostingView(rootView: AnyView(root))
            hosting.sizingOptions = .preferredContentSize
            window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: width, height: 86), styleMask: [.borderless], backing: .buffered, defer: false)
            window.isReleasedWhenClosed = false
            window.contentView = hosting
            settle()
        }

        func tearDown() {
            window.contentView = nil
            window.close()
        }

        var visibleWidth: CGFloat {
            query.textView?.enclosingScrollView?.contentView.bounds.width ?? 0
        }

        func textView() throws -> MultilineTextView {
            for _ in 0 ..< 20 where query.textView == nil {
                settle()
            }
            let textView = try #require(query.textView)
            try #require(textView.enclosingScrollView != nil)
            return textView
        }

        func type(_ text: String, into textView: MultilineTextView) -> [Measurement] {
            text.split(separator: " ").map { word in
                textView.insertText(String(word) + " ", replacementRange: textView.selectedRange())
                settle()
                return measure(textView)
            }
        }

        func measure(_ textView: MultilineTextView) -> Measurement {
            guard let layout = textView.textLayoutManager else {
                return Measurement(textViewWidth: textView.frame.width, usedWidth: 0, lines: 0)
            }
            layout.ensureLayout(for: layout.documentRange)
            var lines = 0
            layout.enumerateTextLayoutFragments(from: layout.documentRange.location, options: [.ensuresLayout]) { fragment in
                lines += fragment.textLineFragments.count
                return true
            }
            return Measurement(textViewWidth: textView.frame.width, usedWidth: layout.usageBoundsForTextContainer.width, lines: lines)
        }

        private func settle() {
            hosting.layoutSubtreeIfNeeded()
            RunLoop.main.run(until: Date().addingTimeInterval(0.02))
            hosting.layoutSubtreeIfNeeded()
        }
    }
}
