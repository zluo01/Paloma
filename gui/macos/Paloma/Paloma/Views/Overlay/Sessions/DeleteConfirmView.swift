//
//  DeleteConfirmView.swift
//  Paloma
//
//  Popover-styled bubble drawn inside the panel window; a real popover
//  would take key status and trigger the panel's hide path.
//

import SwiftUI

struct DeleteConfirmView: View {
    let session: SessionListItem
    let onConfirm: () -> Void
    let onCancel: () -> Void

    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: "trash")
                .font(.system(size: 13))
                .foregroundStyle(.red)
            VStack(alignment: .leading, spacing: 1) {
                Text("Delete \u{201C}\(session.displayTitle)\u{201D}?")
                    .font(.system(size: 13, weight: .medium))
                    .lineLimit(1)
                    .truncationMode(.middle)
                Text("⏎ delete · esc cancel")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
            Button("Cancel", action: onCancel)
                .buttonStyle(.bordered)
            Button("Delete", action: onConfirm)
                .buttonStyle(.borderedProminent)
                .tint(.red)
        }
        .controlSize(.small)
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .background(.thickMaterial, in: RoundedRectangle(cornerRadius: 10))
        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(.quaternary))
        .shadow(color: .black.opacity(0.25), radius: 10, y: 3)
        .padding(.bottom, 8)
        .contentShape(Rectangle())
    }
}
