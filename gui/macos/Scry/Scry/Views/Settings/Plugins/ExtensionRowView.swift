//
//  ExtensionRowView.swift
//  Scry
//

import SwiftUI

struct ExtensionRowView: View {
    let extensionInfo: ExtensionInfo
    let onOpen: () -> Void
    let onEdit: () -> Void
    let onDelete: () -> Void

    var body: some View {
        HStack(spacing: 10) {
            Button(action: onOpen) {
                HStack {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(extensionInfo.name)
                        if !extensionInfo.description.isEmpty {
                            Text(extensionInfo.description)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .lineLimit(2)
                        }
                    }
                    Spacer()
                }
                .contentShape(.rect)
            }
            .buttonStyle(.plain)

            statusIcon

            if extensionInfo.config != nil, extensionInfo.status != .starting {
                Button(action: onEdit) {
                    Image(systemName: "pencil")
                }
                .buttonStyle(.ghostIcon)
                .help("Edit")

                Button(action: onDelete) {
                    Image(systemName: "trash")
                }
                .buttonStyle(.ghostIcon)
                .help("Remove")
            }

            Button(action: onOpen) {
                Image(systemName: "chevron.right")
                    .font(.caption)
            }
            .buttonStyle(.ghostIcon)
            .help("Show Capabilities")
        }
        .padding(.vertical, 2)
    }

    @ViewBuilder
    private var statusIcon: some View {
        switch extensionInfo.status {
        case .running:
            EmptyView()
        case .starting:
            ProgressView().controlSize(.mini)
        case .unhealthy:
            Image(systemName: "exclamationmark.triangle.fill")
                .foregroundStyle(.red)
                .padding(5)
                .help(extensionInfo.error ?? "unknown error")
        }
    }
}
