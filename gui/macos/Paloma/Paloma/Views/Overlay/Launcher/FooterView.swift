//
//  FooterView.swift
//  Paloma
//
//

import SwiftUI

enum FooterState: Equatable {
    case idle
    case search
    case chat(streaming: Bool)
    case session
}

struct FooterView: View {
    let model: LauncherModel
    let state: FooterState
    let onOpenSettings: () -> Void
    let onOpenSession: () -> Void
    let onSelectModel: (ProviderBackendId, String, String) -> Void
    let onStop: () -> Void

    var body: some View {
        HStack(spacing: 6) {
            report
            Spacer()
            if state == .chat(streaming: true) {
                stopButton
            } else {
                controls
            }
        }
        .padding(.horizontal, 14)
        .frame(height: 30)
    }

    @ViewBuilder
    private var report: some View {
        switch state {
        case .idle:
            HealthIndicator(label: "Services", level: model.connectorsHealth)
            HealthIndicator(label: "Plugins", level: model.pluginsHealth)
        case .search:
            hint("↩ Submit")
            separator
            hint("⌘↩ Show actions")
        case .chat:
            hint("⇞ ⇟ Scroll by page")
            separator
            hint("↖ ↘ Scroll to top / bottom")
        case .session:
            hint("↩ Open session")
            separator
            hint("⌦ / ⌘⌫ Delete session")
        }
    }

    private var separator: some View {
        hint("·")
    }

    private func hint(_ text: String) -> some View {
        Text(text)
            .font(.caption)
            .foregroundStyle(.secondary)
            .lineLimit(1)
    }

    @ViewBuilder
    private var controls: some View {
        ModelPickerView(
            connectors: model.connectors,
            preferredModel: model.preferredModel,
            preferredEffort: model.preferredEffort,
            onSelect: onSelectModel
        )
        Button(action: onOpenSettings) {
            Image(systemName: "gearshape")
                .font(.system(size: 12))
        }
        .buttonStyle(.ghostIcon)
        .help("Settings")
        Button(action: onOpenSession) {
            Image(systemName: "clock.arrow.circlepath")
                .font(.system(size: 12))
        }
        .buttonStyle(.ghostIcon)
        .help("Sessions")
    }

    private var stopButton: some View {
        Button(action: onStop) {
            Label("Stop", systemImage: "stop.fill")
        }
        .buttonStyle(.accessoryBarAction)
        .controlSize(.small)
        .help("Interrupt the current response (⌃C)")
    }
}
