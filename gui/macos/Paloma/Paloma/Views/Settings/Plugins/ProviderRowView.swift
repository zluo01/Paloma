//
//  ProviderRowView.swift
//  Paloma
//
//

import SwiftUI

struct ProviderRowView: View {
    let provider: ProviderInfo
    let onEdit: () -> Void
    let onDelete: () -> Void

    var body: some View {
        HStack(spacing: 10) {
            VStack(alignment: .leading, spacing: 2) {
                Text(provider.name)
                if !provider.description.isEmpty {
                    Text(provider.description)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }
            }
            Spacer()

            statusIcon

            if provider.config != nil, provider.status != .starting {
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
        }
        .padding(.vertical, 2)
    }

    @ViewBuilder
    private var statusIcon: some View {
        switch provider.status {
        case .running:
            EmptyView()
        case .starting:
            ProgressView().controlSize(.mini)
        case .unhealthy:
            Image(systemName: "exclamationmark.triangle.fill")
                .foregroundStyle(.red)
                .padding(5)
                .help(provider.error ?? "unknown error")
        }
    }
}
