//
//  ChatView.swift
//  Paloma
//

import AppKit
import SwiftUI

private final class ScrollViewRef {
    weak var anchor: NSView?

    var scrollView: NSScrollView? {
        anchor?.enclosingScrollView
    }
}

private struct ScrollViewAnchor: NSViewRepresentable {
    let ref: ScrollViewRef

    func makeNSView(context _: Context) -> NSView {
        let view = NSView()
        ref.anchor = view
        return view
    }

    func updateNSView(_: NSView, context _: Context) {}
}

struct ChatView: View {
    @Bindable var model: ChatModel

    @State private var scrollViewRef = ScrollViewRef()

    var body: some View {
        ScrollViewReader { proxy in
            CappedScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    ForEach(model.transcript) { section in
                        ChatSectionView(section: section, model: model)
                    }
                    statusRow
                    Color.clear.frame(height: 1).id("chat-bottom")
                }
                .padding(14)
                .background(ScrollViewAnchor(ref: scrollViewRef))
            }
            // Opens restored transcripts at the tail without a follow event.
            .defaultScrollAnchor(.bottom)
            // Follows the tail as the turn streams, unless the user scrolled away.
            .defaultScrollAnchor(.bottom, for: .sizeChanges)
            .onChange(of: model.scrollCommand) { _, _ in
                guard let command = model.takeScrollCommand() else { return }
                scroll(command)
            }
            .onChange(of: model.decisionCursor) {
                // The cursor returning to the input field returns to the tail.
                if let decision = model.selectedDecision {
                    proxy.scrollTo(decision.toolId)
                } else {
                    proxy.scrollTo("chat-bottom")
                }
            }
        }
    }

    private func scroll(_ command: ScrollCommand) {
        guard let scrollView = scrollViewRef.scrollView else { return }
        let currentView = scrollView.contentView
        let distance = currentView.bounds.height - scrollView.verticalPageScroll
        var origin = currentView.bounds.origin
        let limit = max((scrollView.documentView?.frame.height ?? 0) - currentView.bounds.height, 0)
        switch command {
        case .pageUp:
            origin.y -= distance
        case .pageDown:
            origin.y += distance
        case .top:
            origin.y = 0
        case .bottom:
            origin.y = limit
        }
        origin.y = min(max(origin.y, 0), limit)
        currentView.scroll(to: origin)
        scrollView.reflectScrolledClipView(currentView)
    }

    @ViewBuilder
    private var statusRow: some View {
        switch model.chatStatus {
        case .streaming:
            HStack(spacing: 8) {
                ProgressView()
                    .controlSize(.small)
                Text("Thinking…")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        case .cancelled:
            Text("Cancelled")
                .font(.caption)
                .foregroundStyle(.orange)
        case let .failed(message):
            Text(message)
                .font(.caption)
                .foregroundStyle(.red)
        case .idle:
            EmptyView()
        }
    }
}
