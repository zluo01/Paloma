//
//  ChatView.swift
//  Scry
//

import SwiftUI

private struct ScrollMetrics: Equatable {
    var offset: CGFloat
    var container: CGFloat
    var content: CGFloat
}

struct ChatView: View {
    @Bindable var model: ChatModel
    /// Pinned to the tail until the user scrolls away.
    @State private var stuckToBottom = true

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
            }
            // Opens restored transcripts at the tail without a follow event.
            .defaultScrollAnchor(.bottom)
            .onScrollGeometryChange(for: ScrollMetrics.self) { geometry in
                ScrollMetrics(
                    offset: geometry.contentOffset.y,
                    container: geometry.containerSize.height,
                    content: geometry.contentSize.height
                )
            } action: { old, new in
                // Only a real scroll (unchanged content height) re-decides the pin.
                guard old.content == new.content else { return }
                stuckToBottom = new.offset + new.container >= new.content - 24
            }
            .onChange(of: model.chatRevision) {
                if stuckToBottom {
                    proxy.scrollTo("chat-bottom")
                }
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
