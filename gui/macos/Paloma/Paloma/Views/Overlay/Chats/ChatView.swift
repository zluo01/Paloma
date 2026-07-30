//
//  ChatView.swift
//  Paloma
//

import SwiftUI

struct ChatView: View {
    @Bindable var model: ChatModel

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
            // Follows the tail as the turn streams, unless the user scrolled away.
            .defaultScrollAnchor(.bottom, for: .sizeChanges)
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
