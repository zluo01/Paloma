//
//  FooterView.swift
//  Scry
//
//

import SwiftUI

struct FooterView: View {
    let model: LauncherModel
    let onOpenSettings: () -> Void
    let onOpenSession: () -> Void
    let onSelectModel: (ProviderId, String, String) -> Void

    var body: some View {
        HStack(spacing: 12) {
            HealthIndicator(label: "Services", level: model.connectorsHealth)
            HealthIndicator(label: "Plugins", level: model.pluginsHealth)
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
