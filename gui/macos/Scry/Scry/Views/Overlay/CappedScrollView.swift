//
//  CappedScrollView.swift
//  Scry
//

import SwiftUI

/// Self-sizing scroll container capped at the overlay content height.
struct CappedScrollView<Content: View>: View {
    /// Tallest the content area gets before it scrolls internally.
    private static var contentCap: CGFloat {
        340
    }

    @State private var contentHeight: CGFloat = 0
    private let content: Content

    init(@ViewBuilder content: () -> Content) {
        self.content = content()
    }

    var body: some View {
        ScrollView {
            content
                .onGeometryChange(for: CGFloat.self, of: { $0.size.height }) { height in
                    contentHeight = height
                }
        }
        .frame(height: min(contentHeight, Self.contentCap))
    }
}
