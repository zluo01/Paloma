//
//  SessionsView.swift
//  Paloma
//

import SwiftUI

struct SessionsView: View {
    let sessions: [SessionListItem]
    let selection: Int?
    let pendingDeletion: SessionListItem?
    let onRestore: (SessionListItem) -> Void
    let onPendingDelete: (SessionListItem) -> Void
    let onConfirmDelete: () -> Void
    let onCancelDelete: () -> Void

    var body: some View {
        ScrollViewReader { proxy in
            CappedScrollView {
                VStack(alignment: .leading, spacing: 2) {
                    if sessions.isEmpty {
                        Text("No stored sessions")
                            .font(.system(size: 13))
                            .foregroundStyle(.secondary)
                            .padding(14)
                    }
                    ForEach(Array(sessions.enumerated()), id: \.element.sessionId) { index, session in
                        SessionRowView(
                            session: session,
                            selected: index == selection
                        ) {
                            onRestore(session)
                        } onDelete: {
                            onPendingDelete(session)
                        }
                    }
                }
                .padding(8)
                .id("sessions")
            }
            .onChange(of: selection) {
                guard let selection, sessions.indices.contains(selection) else { return }
                // The extremes snap the whole card into view.
                if selection == 0 {
                    proxy.scrollTo("sessions", anchor: .top)
                } else if selection == sessions.count - 1 {
                    proxy.scrollTo("sessions", anchor: .bottom)
                } else {
                    proxy.scrollTo(sessions[selection].sessionId)
                }
            }
            .overlay(alignment: .bottom) {
                if let pendingDeletion {
                    DeleteConfirmView(
                        session: pendingDeletion,
                        onConfirm: onConfirmDelete,
                        onCancel: onCancelDelete
                    )
                    .padding(.horizontal, 16)
                    .transition(.opacity.combined(with: .scale(0.96, anchor: .bottom)))
                }
            }
            .animation(.snappy(duration: 0.15), value: pendingDeletion)
        }
    }
}
