//
//  ChatSectionViewTests.swift
//  PalomaTests
//

import AppKit
@testable import Paloma
import SwiftUI
import Testing

private final class SizeRecorder: @unchecked Sendable {
    var sizes: [CGSize] = []
}

private struct ProbeLayout: Layout {
    let recorder: SizeRecorder

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache _: inout ()) -> CGSize {
        let size = subviews.first?.sizeThatFits(proposal) ?? .zero
        recorder.sizes.append(size)
        return size
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache _: inout ()) {
        subviews.first?.place(at: bounds.origin, proposal: proposal)
    }
}

@MainActor
struct ChatSectionViewTests {
    @Test func givenWideImageWhenLaidOutShouldFitTheProposedWidth() {
        #expect(width(ofSectionWith: NSSize(width: 2560, height: 300), proposed: 640) <= 640)
    }

    @Test func givenTallImageWhenLaidOutShouldFitTheProposedWidth() {
        #expect(width(ofSectionWith: NSSize(width: 800, height: 600), proposed: 640) <= 640)
    }

    private func width(ofSectionWith imageSize: NSSize, proposed: CGFloat) -> CGFloat {
        let image = NSImage(size: imageSize, flipped: false) { rect in
            NSColor.systemTeal.setFill()
            rect.fill()
            return true
        }
        let recorder = SizeRecorder()
        let section = ChatSection.user(id: 1, text: "look [Image #1]", images: [1: image])
        let host = NSHostingView(rootView: ProbeLayout(recorder: recorder) {
            ChatSectionView(section: section, model: ChatModel())
        }
        .frame(width: proposed))
        host.frame = NSRect(x: 0, y: 0, width: proposed, height: 400)
        host.layoutSubtreeIfNeeded()
        _ = host.fittingSize
        return recorder.sizes.map(\.width).max() ?? 0
    }
}
