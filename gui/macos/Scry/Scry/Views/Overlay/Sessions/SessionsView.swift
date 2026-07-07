//
//  SessionsView.swift
//  Scry
//

import SwiftUI

struct SessionsView: View {
    let sessions: [SessionListItem]
    let selection: Int?
    let onRestore: (SessionListItem) -> Void
    let onDelete: (SessionListItem) -> Void

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
                            onDelete(session)
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
        }
    }
}
