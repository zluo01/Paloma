//
//  View+SearchRow.swift
//  Paloma
//

import SwiftUI

extension View {
    func searchRow(selected: Bool, hovering: Binding<Bool>, onTap: @escaping () -> Void) -> some View {
        modifier(SearchRowModifier(hovering: hovering, selected: selected, onTap: onTap))
    }
}

private struct SearchRowModifier: ViewModifier {
    @Binding var hovering: Bool
    let selected: Bool
    let onTap: () -> Void

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
