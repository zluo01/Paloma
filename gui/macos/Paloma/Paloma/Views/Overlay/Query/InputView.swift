//
//  InputView.swift
//  Paloma
//
//

import SwiftUI

struct InputView: NSViewRepresentable {
    let query: QueryModel
    @Binding var composing: Bool
    let onSubmit: () -> Void
    let onNavigate: (Int) -> Void
    let onEscape: () -> Void

    func makeCoordinator() -> Coordinator {
        Coordinator(self)
    }

    func makeNSView(context: Context) -> NSScrollView {
        let textView = MultilineTextView.make()
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

    func updateNSView(_: NSScrollView, context: Context) {
        context.coordinator.parent = self
    }

    func sizeThatFits(_ proposal: ProposedViewSize, nsView scrollView: NSScrollView, context _: Context) -> CGSize? {
        guard let textView = scrollView.documentView as? MultilineTextView else { return nil }
        let width = proposal.width ?? textView.frame.width
        if textView.frame.width != width {
            textView.frame.size.width = width
        }
        let cap = MultilineTextView.lineHeight * CGFloat(MultilineTextView.maxLines)
        return CGSize(width: width, height: ceil(min(textView.contentHeight, cap)))
    }

    @MainActor
    final class Coordinator: NSObject, NSTextViewDelegate {
        var parent: InputView
        private weak var textView: MultilineTextView?
        private var observers: [NSObjectProtocol] = []

        init(_ parent: InputView) {
            self.parent = parent
        }

        deinit {
            observers.forEach(NotificationCenter.default.removeObserver)
        }

        func attach(_ textView: MultilineTextView) {
            self.textView = textView
            parent.query.textView = textView
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
            parent.query.sync()
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
