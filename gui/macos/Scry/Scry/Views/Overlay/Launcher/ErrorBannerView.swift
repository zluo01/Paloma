//
//  ErrorBannerView.swift
//  Scry
//
//

import SwiftUI

/// Auto-dismisses after a few seconds; a newer error restarts the timer.
struct ErrorBannerView: View {
    let error: OperationError
    let onDismiss: () -> Void

    var body: some View {
        Divider()
        HStack(spacing: 6) {
            Image(systemName: "exclamationmark.triangle.fill")
            Text("\(error.title): \(error.message)")
                .lineLimit(1)
        }
        .font(.caption)
        .foregroundStyle(.red)
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, 18)
        .padding(.vertical, 3)
        .task(id: error.id) {
            guard await (try? Task.sleep(for: .seconds(4))) != nil else { return }
            onDismiss()
        }
    }
}
