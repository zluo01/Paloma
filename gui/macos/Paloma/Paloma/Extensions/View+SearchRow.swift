//
//  View+SearchRow.swift
//  Paloma
//

import SwiftUI

extension View {
    func searchRow(selected: Bool, onTap: @escaping () -> Void) -> some View {
        padding(.horizontal, 8)
            .padding(.vertical, 5)
            .rowHighlight(selected)
            .contentShape(Rectangle())
            .onTapGesture(perform: onTap)
    }
}
