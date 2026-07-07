//
//  GhostIconButtonStyle.swift
//  Scry
//

import SwiftUI

struct GhostIconButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        GhostIconButton(configuration: configuration)
    }

    private struct GhostIconButton: View {
        let configuration: Configuration
        @State private var hovering = false

        var body: some View {
            configuration.label
                .foregroundStyle(.secondary)
                .padding(5)
                .background(.primary.opacity(tint), in: Circle())
                .contentShape(Circle())
                .animation(.easeOut(duration: 0.1), value: hovering)
                .onHover { hovering = $0 }
        }

        private var tint: Double {
            if configuration.isPressed {
                0.16
            } else if hovering {
                0.08
            } else {
                0
            }
        }
    }
}

extension ButtonStyle where Self == GhostIconButtonStyle {
    static var ghostIcon: GhostIconButtonStyle {
        GhostIconButtonStyle()
    }
}
