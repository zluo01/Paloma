//
//  ReasoningView.swift
//  Paloma
//
//

import SwiftUI

struct ReasoningView: View {
    let text: String
    @State private var expanded = false

    var body: some View {
        DisclosureGroup(isExpanded: $expanded) {
            Text(text)
                .font(.system(size: 12))
                .foregroundStyle(.secondary)
                .textSelection(.enabled)
        } label: {
            Text("Thinking")
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)
                .contentShape(Rectangle())
                .onTapGesture {
                    withAnimation {
                        expanded.toggle()
                    }
                }
        }
    }
}
