//
//  FooterView.swift
//  Scry
//
//

import SwiftUI

struct FooterView: View {
    let model: LauncherModel
    let mode: OverlayMode
    let onOpenSettings: () -> Void
    let onOpenSession: () -> Void
    let onSelectModel: (ProviderBackendId, String, String) -> Void

    private var hints: String {
        switch mode {
        case .search: "⏎ open · ⌘⏎ actions · ⇧↓ sessions"
        case .chat: "⏎ send · ⌃C stop · ⇧↓ sessions"
        case .session: "⏎ restore · ⌦ delete"
        }
    }

    var body: some View {
        HStack(spacing: 12) {
            HealthIndicator(label: "Services", level: model.connectorsHealth)
            HealthIndicator(label: "Plugins", level: model.pluginsHealth)
            Spacer()
            Text(hints)
                .font(.caption2)
                .foregroundStyle(.tertiary)
                .lineLimit(1)
            Spacer()
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
        .padding(.horizontal, 14)
        .frame(height: 30)
    }
}
