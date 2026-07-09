//
//  ChatView.swift
//  Scry
//

import SwiftUI

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
            .onScrollPhaseChange { _, newPhase, context in
                // A user scroll suspends the follow; where it settles decides.
                guard newPhase != .animating else { return }
                if newPhase == .idle {
                    let geometry = context.geometry
                    let pinned = geometry.contentOffset.y + geometry.containerSize.height
                        >= geometry.contentSize.height - 24
                    if stuckToBottom != pinned {
                        stuckToBottom = pinned
                    }
                } else if stuckToBottom {
                    stuckToBottom = false
                }
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
