//
//  HealthIndicator.swift
//  Scry
//
//

import SwiftUI

struct HealthIndicator: View {
    let label: String
    let level: HealthLevel

    var body: some View {
        HStack(spacing: 4) {
            Circle()
                .fill(color(for: level))
                .frame(width: 7, height: 7)
            Text(label)
                .font(.caption2)
                .foregroundStyle(.secondary)
        }
        .help("\(label): \(describe(level))")
    }

    private func color(for level: HealthLevel) -> Color {
        switch level {
        case .inactive: .gray.opacity(0.5)
        case .healthy: .green
        case .degraded: .yellow
        case .down: .red
        }
    }

    private func describe(_ level: HealthLevel) -> String {
        switch level {
        case .inactive: "not configured"
        case .healthy: "healthy"
        case .degraded: "degraded"
        case .down: "down"
        }
    }
}
