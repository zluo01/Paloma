//
//  PluginDialogTests.swift
//  PalomaTests
//

import AppKit
@testable import Paloma
import SwiftUI
import Testing

/// Drives the hosted dialog to check its submit button follows `canSubmit`;
/// the cases themselves are covered in `PluginDialogStateTests`.
@MainActor
@Suite(.serialized)
struct PluginDialogTests {
    @Test func givenSubmittableFormWhenPressingReturnShouldSubmit() {
        let host = Host(Self.form())
        defer { host.tearDown() }

        #expect(host.pressReturn())
        #expect(host.isSubmitting)
    }

    @Test func givenLocalServerWithoutArgumentsWhenPressingReturnShouldSubmit() {
        var state = PluginDialogState(.mcp)
        state.name = "files"
        state.command = "server-files"
        let host = Host(state)
        defer { host.tearDown() }

        #expect(host.pressReturn())
        #expect(host.isSubmitting)
    }

    /// An empty name is refused by `canSubmit` alone: nothing else in the
    /// submit path would stop it.
    @Test func givenBlockedFormWhenPressingReturnShouldNotSubmit() {
        let host = Host(Self.form { $0.name = "" })
        defer { host.tearDown() }

        #expect(!host.pressReturn())
        #expect(!host.isSubmitting)
    }

    private static func form(_ change: (inout PluginDialogState) -> Void = { _ in }) -> PluginDialogState {
        var state = PluginDialogState(.mcp)
        state.name = "linear"
        state.isRemote = true
        state.url = "https://mcp.example.com/mcp"
        change(&state)
        return state
    }

    @MainActor
    final class Host {
        let window: NSWindow
        let hosting: NSHostingView<PluginDialog>

        init(_ state: PluginDialogState) {
            hosting = NSHostingView(rootView: PluginDialog(model: PluginModel(), onClose: {}, state: state))
            window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 480, height: 600), styleMask: [.titled], backing: .buffered, defer: false)
            window.isReleasedWhenClosed = false
            window.contentView = hosting
            settle()
        }

        func tearDown() {
            window.contentView = nil
            window.close()
        }

        /// Whether the dialog's default button took the key.
        func pressReturn() -> Bool {
            let event = NSEvent.keyEvent(
                with: .keyDown, location: .zero, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
                windowNumber: window.windowNumber, context: nil, characters: "\r", charactersIgnoringModifiers: "\r",
                isARepeat: false, keyCode: 36
            )
            let handled = event.map(window.performKeyEquivalent(with:)) ?? false
            settle()
            return handled
        }

        /// The dialog disables its fields while a submit is in flight.
        var isSubmitting: Bool {
            let fields = textFields(in: hosting)
            return !fields.isEmpty && fields.allSatisfy { !$0.isEnabled }
        }

        private func textFields(in view: NSView) -> [NSTextField] {
            ((view as? NSTextField).map { [$0] } ?? []) + view.subviews.flatMap(textFields(in:))
        }

        private func settle() {
            hosting.layoutSubtreeIfNeeded()
            RunLoop.main.run(until: Date().addingTimeInterval(0.05))
            hosting.layoutSubtreeIfNeeded()
        }
    }
}
