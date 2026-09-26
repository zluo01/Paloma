//
//  WrapLayoutTests.swift
//  PalomaTests
//

import AppKit
@testable import Paloma
import SwiftUI
import Testing

@MainActor
struct WrapLayoutTests {
    @Test func givenItemsThatFitWhenLayingOutShouldUseOneRow() {
        #expect(height(of: 3, itemWidth: 100, in: 400) == 50)
    }

    @Test func givenItemsWiderThanTheRowWhenLayingOutShouldWrapOntoTheNextRow() {
        let twoRowsWithSpacing: CGFloat = 50 + 6 + 50
        #expect(height(of: 3, itemWidth: 100, in: 250) == twoRowsWithSpacing)
    }

    @Test func givenOneItemWiderThanTheRowWhenLayingOutShouldStillPlaceIt() {
        #expect(height(of: 1, itemWidth: 100, in: 80) == 50)
    }

    private func height(of count: Int, itemWidth: CGFloat, in width: CGFloat) -> CGFloat {
        let view = WrapLayout(spacing: 6) {
            ForEach(0 ..< count, id: \.self) { _ in
                Color.teal.frame(width: itemWidth, height: 50)
            }
        }
        .frame(width: width, alignment: .leading)
        return NSHostingView(rootView: view).fittingSize.height
    }
}
