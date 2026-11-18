//
//  CappedScrollViewTests.swift
//  PalomaTests
//

import AppKit
import Observation
@testable import Paloma
import SwiftUI
import Testing

@MainActor
@Suite(.serialized)
struct CappedScrollViewTests {
    private static let cap: CGFloat = 340

    // MARK: - Sizing

    @Test(arguments: [0, 40, 339, 340, 341, 900] as [CGFloat])
    func givenContentWhenLaidOutShouldBeAsTallAsTheContentUpToTheCap(content: CGFloat) {
        let host = Host(CappedScrollView { Color.clear.frame(height: content) })
        defer { host.tearDown() }

        #expect(host.height == min(content, Self.cap))
    }

    @Test func givenContentThatGrowsWhenUpdatedShouldFollowItUntilTheCap() {
        let inputs = Inputs()
        let host = Host(CappedScrollView { ResizableContent(inputs: inputs) })
        defer { host.tearDown() }

        var heights: [CGFloat] = []
        for content in [20, 150, 339, 500, 1200, 90] as [CGFloat] {
            inputs.height = content
            host.settle()
            heights.append(host.height)
        }
        #expect(heights == [20, 150, 339, 340, 340, 90])
    }

    @Test func givenContentTallerThanTheCapWhenLaidOutShouldScrollInsideTheCap() throws {
        let host = Host(CappedScrollView { Color.clear.frame(height: 900) })
        defer { host.tearDown() }

        #expect(try host.visibleHeight() == Self.cap)
        #expect(try host.bottom() == 900 - Self.cap)
        #expect(try host.offset() == 0)
    }

    // MARK: - Search results

    @Test func givenFewResultsWhenShownShouldBeOnlyAsTallAsTheResults() {
        let few = Host(Self.searchView(Inputs(), rows: 2))
        defer { few.tearDown() }
        let many = Host(Self.searchView(Inputs(), rows: 40))
        defer { many.tearDown() }

        #expect(few.height > 0)
        #expect(few.height < Self.cap)
        #expect(many.height == Self.cap)
    }

    @Test func givenManyResultsWhenTheSelectionMovesShouldScrollItIntoView() throws {
        let inputs = Inputs()
        let host = Host(Self.searchView(inputs, rows: 40))
        defer { host.tearDown() }
        let bottom = try host.bottom()
        try #require(bottom > 0)

        var offsets: [CGFloat] = []
        for selection in [39, 20, 0] {
            inputs.selection = selection
            host.settle()
            try offsets.append(host.offset())
        }
        // The chat row sits below the last result, so that one snaps to the very bottom.
        inputs.chatRowSelected = true
        inputs.selection = 40
        host.settle()
        try offsets.append(host.offset())

        #expect(offsets[0] > offsets[1])
        #expect(offsets[1] > 0)
        #expect(offsets[2] == 0)
        #expect(offsets[3] == bottom)
    }

    // MARK: - Sessions

    @Test func givenManySessionsWhenTheSelectionMovesShouldScrollItIntoView() throws {
        let inputs = Inputs()
        let host = Host(Self.sessionsView(inputs, rows: 30))
        defer { host.tearDown() }
        #expect(host.height == Self.cap)
        let bottom = try host.bottom()

        var offsets: [CGFloat] = []
        for selection in [29, 15, 0] {
            inputs.selection = selection
            host.settle()
            try offsets.append(host.offset())
        }
        #expect(offsets[0] == bottom)
        #expect(offsets[1] > 0)
        #expect(offsets[1] < bottom)
        #expect(offsets[2] == 0)
    }

    @Test func givenFewSessionsWhenShownShouldBeOnlyAsTallAsTheSessions() {
        let host = Host(Self.sessionsView(Inputs(), rows: 3))
        defer { host.tearDown() }

        #expect(host.height > 0)
        #expect(host.height < Self.cap)
    }

    // MARK: - Chat

    @Test func givenLongAnswerWhenStreamedShouldGrowToTheCapAndStayAtTheTail() throws {
        let model = ChatModel()
        let host = Host(ChatView(model: model))
        defer { host.tearDown() }
        let backend = ProviderBackendId(providerId: "Test", backendId: "test")

        var heights: [CGFloat] = []
        for _ in 0 ..< 40 {
            model.render(.chat(event: .textDelta(providerBackendId: backend, text: "Another line of the answer.\n\n")))
            host.settle()
            heights.append(host.height)
        }
        #expect(try #require(heights.first) < Self.cap)
        #expect(heights == heights.sorted())
        #expect(heights.last == Self.cap)
        #expect(try host.offset() == host.bottom())
        #expect(try host.bottom() > 0)
    }

    // MARK: - Fixtures

    @Observable
    final class Inputs {
        var height: CGFloat = 0
        var selection = 0
        var chatRowSelected = false
    }

    private struct ResizableContent: View {
        let inputs: Inputs

        var body: some View {
            Color.clear.frame(height: inputs.height)
        }
    }

    private struct SearchList: View {
        let inputs: Inputs
        let sections: [QueryResponse]

        var body: some View {
            SearchView(
                query: "query",
                sections: sections,
                bases: [0],
                selection: inputs.selection,
                panelSelection: nil,
                selectedItem: nil,
                chatRowSelected: inputs.chatRowSelected,
                onEvent: { _ in }
            )
        }
    }

    private struct SessionsList: View {
        let inputs: Inputs
        let sessions: [SessionListItem]

        var body: some View {
            SessionsView(
                sessions: sessions,
                selection: inputs.selection,
                pendingDeletion: nil,
                onRestore: { _ in },
                onPendingDelete: { _ in },
                onConfirmDelete: {},
                onCancelDelete: {}
            )
        }
    }

    private static func searchView(_ inputs: Inputs, rows: Int) -> some View {
        let items = (0 ..< rows).map { index in
            Item(title: "Result \(index)", subtitle: "Subtitle \(index)", icon: nil, actions: [Action(label: "Open", params: [], primary: true)])
        }
        let section = QueryResponse(
            extensionCapabilityId: ExtensionCapabilityId(extensionId: "ext", capabilityId: "cap"), name: "Results", items: items
        )
        return SearchList(inputs: inputs, sections: [section])
    }

    private static func sessionsView(_ inputs: Inputs, rows: Int) -> some View {
        let sessions = (0 ..< rows).map { index in
            SessionListItem(sessionId: "session-\(index)", title: "Session \(index)", lastUpdate: 1_700_000_000)
        }
        return SessionsList(inputs: inputs, sessions: sessions)
    }

    @MainActor
    final class Host {
        let window: NSWindow
        let hosting: NSHostingView<AnyView>

        init(_ content: some View) {
            hosting = NSHostingView(rootView: AnyView(content.frame(width: 640)))
            window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 640, height: 1000), styleMask: [.borderless], backing: .buffered, defer: false)
            window.isReleasedWhenClosed = false
            window.contentView = hosting
            settle()
        }

        func tearDown() {
            window.contentView = nil
            window.close()
        }

        func settle() {
            for _ in 0 ..< 3 {
                hosting.layoutSubtreeIfNeeded()
                RunLoop.main.run(until: Date().addingTimeInterval(0.02))
            }
            hosting.layoutSubtreeIfNeeded()
        }

        var height: CGFloat {
            hosting.fittingSize.height
        }

        func offset() throws -> CGFloat {
            try scrollView().contentView.bounds.origin.y
        }

        func visibleHeight() throws -> CGFloat {
            try scrollView().contentView.bounds.height
        }

        func bottom() throws -> CGFloat {
            let scrollView = try scrollView()
            return max((scrollView.documentView?.frame.height ?? 0) - scrollView.contentView.bounds.height, 0)
        }

        private func scrollView() throws -> NSScrollView {
            // The window has to match the fitting size for the scroll view to see its real viewport.
            window.setContentSize(hosting.fittingSize)
            settle()
            return try #require(Self.scrollViews(in: hosting).first)
        }

        private static func scrollViews(in view: NSView) -> [NSScrollView] {
            ((view as? NSScrollView).map { [$0] } ?? []) + view.subviews.flatMap(scrollViews(in:))
        }
    }
}
