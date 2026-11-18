//
//  CappedScrollView.swift
//  Paloma
//

import SwiftUI

/// Self-sizing scroll container capped at the overlay content height.
struct CappedScrollView<Content: View>: View {
    /// Tallest the content area gets before it scrolls internally.
    private static var contentCap: CGFloat {
        340
    }

    private let content: Content

    init(@ViewBuilder content: () -> Content) {
        self.content = content()
    }

    var body: some View {
        ScrollView {
            content
        }
        .frame(maxHeight: Self.contentCap)
        .fixedSize(horizontal: false, vertical: true)
    }
}
