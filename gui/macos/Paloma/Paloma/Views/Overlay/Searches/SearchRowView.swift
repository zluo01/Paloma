//
//  SearchRowView.swift
//  Paloma
//
//

import SwiftUI

struct SearchRowView: View {
    let item: Item
    let index: Int
    let selected: Bool
    let actionHint: Bool
    let onEvent: (SearchEvent) -> Void

    @State private var hovering = false

    var body: some View {
        HStack(spacing: 10) {
            IconView(icon: item.icon)
            VStack(alignment: .leading, spacing: 1) {
                Text(item.title)
                    .font(.system(size: 14))
                    .lineLimit(1)
                if let subtitle = item.subtitle, !subtitle.isEmpty {
                    Text(subtitle)
                        .font(.system(size: 11))
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
            }
            Spacer(minLength: 0)
            if hovering, item.actions.count > 1 {
                Button {
                    onEvent(.showActions(index: index))
                } label: {
                    Image(systemName: "ellipsis")
                        .font(.system(size: 14, weight: .semibold))
                        .frame(width: 18, height: 18)
                }
                .buttonStyle(.ghostIcon)
                .help("Show actions")
            } else if actionHint {
                Text("⌘⏎")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
        }
        .searchRow(selected: selected, hovering: $hovering) {
            onEvent(.action(index: index))
        }
        .anchorPreference(key: SelectedRowBounds.self, value: .bounds) { anchor in
            selected ? anchor : nil
        }
    }
}
