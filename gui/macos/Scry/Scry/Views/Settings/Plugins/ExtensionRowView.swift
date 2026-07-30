//
//  ExtensionRowView.swift
//  Scry
//

import SwiftUI

struct ExtensionRowView: View {
    let extensionInfo: ExtensionInfo
    let onOpen: () -> Void
    let onEdit: () -> Void
    let onToggle: (_ disabled: Bool) -> Void
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
            // Built-ins have no config row to disable.
            if let config = extensionInfo.config {
                Toggle(
                    "Toggle Extension Plugin",
                    isOn: .init(
                        get: { !config.disabled },
                        set: { enabled in
                            onToggle(!enabled)
                        }
                    )
                )
                .toggleStyle(.switch)
                .controlSize(.small)
                .labelsHidden()
            }
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
