//
//  QueryView.swift
//  Paloma
//
//

import AppKit
import SwiftUI

struct QueryView: View {
    @Binding var query: String
    let mode: OverlayMode
    let onSearch: (String) -> Void
    let onSubmit: () -> Void
    let onNavigate: (Int) -> Void
    let onEscape: () -> Void

    /// indicator to tell if current input through IME
    @State private var composing = false

    private var placeholder: String {
        switch mode {
        case .search: "Search, or ask anything…"
        case .chat: "Reply…"
        case .session: "Search sessions…"
        }
    }

    private var icon: String {
        switch mode {
        case .search: "magnifyingglass"
        case .chat: "sparkles"
        case .session: "clock.arrow.circlepath"
        }
    }

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: icon)
                .font(.system(size: 20, weight: .light))
                .foregroundStyle(.secondary)
                .padding(.top, 2)
            ZStack(alignment: .topLeading) {
                if query.isEmpty, !composing {
                    Text(placeholder)
                        .font(.system(size: 22, weight: .light))
                        .foregroundStyle(Color(nsColor: .placeholderTextColor))
                }
                ComposerView(
                    text: $query,
                    composing: $composing,
                    onSubmit: onSubmit,
                    onNavigate: onNavigate,
                    onEscape: onEscape
                )
            }
        }
        .padding(.horizontal, 18)
        .padding(.vertical, 16)
        .task(id: query) {
            if !query.isEmpty {
                guard await (try? Task.sleep(for: .milliseconds(150))) != nil else { return }
            }
            onSearch(query)
        }
    }
}

private struct ComposerView: NSViewRepresentable {
    @Binding var text: String
    @Binding var composing: Bool
    let onSubmit: () -> Void
    let onNavigate: (Int) -> Void
    let onEscape: () -> Void

    func makeCoordinator() -> Coordinator {
        Coordinator(self)
    }

    func makeNSView(context: Context) -> NSScrollView {
        let textView = ComposerTextView.make()
        textView.delegate = context.coordinator

        let scrollView = NSScrollView()
        scrollView.documentView = textView
        scrollView.drawsBackground = false
        scrollView.hasVerticalScroller = true
        scrollView.autohidesScrollers = true
        scrollView.scrollerStyle = .overlay
        scrollView.hasHorizontalScroller = false

        context.coordinator.attach(textView)
        return scrollView
    }

    func updateNSView(_ scrollView: NSScrollView, context: Context) {
        context.coordinator.parent = self
        if let textView = scrollView.documentView as? ComposerTextView,
           !textView.hasMarkedText(), textView.string != text
        {
            textView.string = text
        }
        context.coordinator.invalidateSize()
    }

    func sizeThatFits(_ proposal: ProposedViewSize, nsView scrollView: NSScrollView, context _: Context) -> CGSize? {
        guard let textView = scrollView.documentView as? ComposerTextView else { return nil }
        let width = proposal.width ?? textView.frame.width
        if textView.frame.width != width {
            textView.frame.size.width = width
        }
        let cap = ComposerTextView.lineHeight * CGFloat(ComposerTextView.maxLines)
        return CGSize(width: width, height: ceil(min(textView.contentHeight, cap)))
    }

    @MainActor
    final class Coordinator: NSObject, NSTextViewDelegate {
        var parent: ComposerView
        private weak var textView: ComposerTextView?
        private var observers: [NSObjectProtocol] = []

        init(_ parent: ComposerView) {
            self.parent = parent
        }

        deinit {
            observers.forEach(NotificationCenter.default.removeObserver)
        }

        func attach(_ textView: ComposerTextView) {
            self.textView = textView
            textView.onSubmit = { [weak self] in self?.parent.onSubmit() }
            textView.onNavigate = { [weak self] delta in self?.parent.onNavigate(delta) }
            textView.onEscape = { [weak self] in self?.parent.onEscape() }
            textView.onMarkedTextChange = { [weak self] in self?.syncComposing() }

            // auto focus on the text view when launcher view is focused
            observers.append(NotificationCenter.default.addObserver(
                forName: NSWindow.didBecomeKeyNotification, object: nil, queue: .main
            ) { [weak self] note in
                MainActor.assumeIsolated {
                    guard note.object is PalomaPanel else { return }
                    guard let textView = self?.textView else { return }
                    textView.window?.makeFirstResponder(textView)
                }
            })
            DispatchQueue.main.async {
                textView.window?.makeFirstResponder(textView)
            }
        }

        func textDidChange(_: Notification) {
            guard let textView else { return }
            parent.text = textView.string
            invalidateSize()
        }

        private func syncComposing() {
            let composing = textView?.hasMarkedText() ?? false
            if parent.composing != composing {
                parent.composing = composing
            }
        }

        func invalidateSize() {
            textView?.enclosingScrollView?.invalidateIntrinsicContentSize()
        }
    }
}

final class ComposerTextView: NSTextView {
    static let font = NSFont.systemFont(ofSize: 22, weight: .light)
    static let maxLines = 6
    static let lineHeight = NSLayoutManager().defaultLineHeight(for: font)

    var onSubmit: () -> Void = {}
    var onNavigate: (Int) -> Void = { _ in }
    var onEscape: () -> Void = {}
    var onMarkedTextChange: () -> Void = {}

    static func make() -> ComposerTextView {
        let textView = ComposerTextView(usingTextLayoutManager: true)
        textView.isRichText = false
        textView.importsGraphics = false
        textView.allowsUndo = true
        textView.usesFontPanel = false
        textView.usesFindBar = false
        textView.isContinuousSpellCheckingEnabled = false
        textView.isAutomaticQuoteSubstitutionEnabled = false
        textView.isAutomaticDashSubstitutionEnabled = false
        textView.isAutomaticTextCompletionEnabled = false
        textView.drawsBackground = false
        textView.font = font
        textView.textColor = .labelColor
        textView.textContainerInset = .zero
        textView.textContainer?.lineFragmentPadding = 0
        textView.textContainer?.widthTracksTextView = true
        textView.isVerticallyResizable = true
        textView.isHorizontallyResizable = false
        textView.autoresizingMask = [.width]
        textView.minSize = NSSize(width: 0, height: lineHeight)
        textView.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        textView.setAccessibilityLabel("Query")
        return textView
    }

    var contentHeight: CGFloat {
        guard let layout = textLayoutManager else { return Self.lineHeight }
        layout.ensureLayout(for: layout.documentRange)
        return max(layout.usageBoundsForTextContainer.height, Self.lineHeight)
    }

    override func setMarkedText(_ string: Any, selectedRange: NSRange, replacementRange: NSRange) {
        super.setMarkedText(string, selectedRange: selectedRange, replacementRange: replacementRange)
        onMarkedTextChange()
    }

    override func unmarkText() {
        super.unmarkText()
        onMarkedTextChange()
    }

    override func insertText(_ string: Any, replacementRange: NSRange) {
        super.insertText(string, replacementRange: replacementRange)
        onMarkedTextChange()
    }

    override func doCommand(by selector: Selector) {
        switch selector {
        case #selector(insertNewline(_:)):
            // intercept Enter and only keep Shift+Enter as insert new line
            if NSApp.currentEvent?.modifierFlags.contains(.shift) == true {
                super.doCommand(by: selector)
            } else {
                onSubmit()
            }
        case #selector(moveUp(_:)):
            if isOnEdge(.up) {
                onNavigate(-1)
            } else {
                super.doCommand(by: selector)
            }
        case #selector(moveDown(_:)):
            if isOnEdge(.down) {
                onNavigate(1)
            } else {
                super.doCommand(by: selector)
            }
        case #selector(cancelOperation(_:)):
            onEscape()
        case #selector(insertTab(_:)):
            break
        default:
            super.doCommand(by: selector)
        }
    }

    func isOnEdge(_ direction: NSTextSelectionNavigation.Direction) -> Bool {
        guard let layout = textLayoutManager,
              let selection = layout.textSelections.first,
              let destination = layout.textSelectionNavigation.destinationSelection(
                  for: selection,
                  direction: direction,
                  destination: .character,
                  extending: false,
                  confined: false
              ),
              let current = caretY(of: selection, in: layout),
              let target = caretY(of: destination, in: layout)
        else {
            return false
        }
        return abs(target - current) < 0.5
    }

    /// Get the current line for caret
    func caretY(of selection: NSTextSelection, in layout: NSTextLayoutManager) -> CGFloat? {
        guard let location = selection.textRanges.first?.location else { return nil }
        var y: CGFloat?
        layout.enumerateTextSegments(
            in: NSTextRange(location: location),
            type: .selection,
            options: selection.affinity == .upstream ? .upstreamAffinity : []
        ) { _, frame, _, _ in
            y = frame.minY
            return false
        }
        return y
    }
}
