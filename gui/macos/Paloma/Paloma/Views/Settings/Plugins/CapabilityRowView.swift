//
//  CapabilityRowView.swift
//  Paloma
//
//  One row for both an extension capability and an MCP tool; core reports
//  each as a CapabilityInfo.
//

import SwiftUI

struct CapabilityRowView: View {
    let capability: CapabilityInfo
    let facet: CapabilityFacet
    /// A disabled plugin disables everything under it; the switch follows.
    let isPluginDisabled: Bool
    let onToggle: (_ disabled: Bool) -> Void

    var body: some View {
        HStack(spacing: 10) {
            VStack(alignment: .leading, spacing: 2) {
                Text(capability.id)
                if !capability.description.isEmpty {
                    Text(capability.description)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            Spacer()

            Toggle(
                "Toggle Capability \(capability.id)",
                isOn: .init(
                    get: { !capability.facets.contains { $0.facet == facet && $0.disabled } },
                    set: { enabled in onToggle(!enabled) }
                )
            )
            .toggleStyle(.switch)
            .controlSize(.small)
            .labelsHidden()
            .disabled(isPluginDisabled)
        }
        .padding(.vertical, 2)
    }
}
