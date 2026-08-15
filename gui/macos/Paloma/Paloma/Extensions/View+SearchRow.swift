//
//  View+SearchRow.swift
//  Paloma
//

import SwiftUI

extension View {
    func searchRow(selected: Bool, onTap: @escaping () -> Void) -> some View {
        modifier(SearchRowModifier(selected: selected, onTap: onTap))
    }
}

private struct SearchRowModifier: ViewModifier {
    let selected: Bool
    let onTap: () -> Void
    @State private var hovering = false

    func body(content: Content) -> some View {
        content
            .padding(.horizontal, 8)
            .padding(.vertical, 5)
            .rowHighlight(selected || hovering)
            .contentShape(Rectangle())
            .onHover { hovering = $0 }
            .onTapGesture(perform: onTap)
    }
}
