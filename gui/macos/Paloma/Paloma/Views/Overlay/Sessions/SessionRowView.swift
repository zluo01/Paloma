//
//  SessionRowView.swift
//  Paloma
//
//

import SwiftUI

struct SessionRowView: View {
    let session: SessionListItem
    let selected: Bool
    let onRestore: () -> Void
    let onDelete: () -> Void
    @State private var hovering = false

    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: "bubble.left.and.bubble.right")
                .font(.system(size: 14))
                .foregroundStyle(.secondary)
                .frame(width: 28)
            VStack(alignment: .leading, spacing: 1) {
                Text(session.displayTitle)
                    .font(.system(size: 14))
                    .lineLimit(1)
                Text(relativeDate)
                    .font(.system(size: 11))
                    .foregroundStyle(.secondary)
            }
            Spacer(minLength: 0)
            if hovering {
                Button(action: onDelete) {
                    Image(systemName: "trash")
                        .font(.system(size: 12))
                }
                .buttonStyle(.ghostIcon)
            }
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 6)
        .rowHighlight(selected || hovering)
        .contentShape(Rectangle())
        .onHover { hovering = $0 }
        .onTapGesture(perform: onRestore)
    }

    private var relativeDate: String {
        Date(timeIntervalSince1970: TimeInterval(session.lastUpdate))
            .formatted(.relative(presentation: .named))
    }
}
