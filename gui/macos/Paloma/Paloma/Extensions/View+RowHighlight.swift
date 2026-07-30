//
//  View+RowHighlight.swift
//  Paloma
//

import SwiftUI

extension View {
    func rowHighlight(_ active: Bool, cornerRadius: CGFloat = 8) -> some View {
        background(
            active ? AnyShapeStyle(.selection) : AnyShapeStyle(.clear),
            in: RoundedRectangle(cornerRadius: cornerRadius)
        )
    }
}
