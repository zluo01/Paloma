//
//  AppDelegate.swift
//  Scry
//

import AppKit
import KeyboardShortcuts
import SwiftUI

extension KeyboardShortcuts.Name {
    static let toggleScry = Self("toggleScry", default: .init(.space, modifiers: [.option]))
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    private var panel: ScryPanel?
    private var settingsWindow: NSWindow?
    private let launcher = LauncherModel()

    func applicationDidFinishLaunching(_: Notification) {
        NSApp.setActivationPolicy(.accessory)
        // Single instance only: hand off to an already running Scry.
        if let bundleId = Bundle.main.bundleIdentifier {
            let others = NSRunningApplication.runningApplications(withBundleIdentifier: bundleId)
                .filter { $0.processIdentifier != ProcessInfo.processInfo.processIdentifier }
            if let other = others.first {
                other.activate()
                NSApp.terminate(nil)
                return
            }
        }
        Task {
            switch await CoreClient.shared.bootstrap() {
            case .success:
                KeyboardShortcuts.onKeyUp(for: .toggleScry) { [weak self] in
                    self?.togglePanel()
                }
            case let .failure(error):
                presentStartupFailure(error)
            }
        }
    }

    private func presentStartupFailure(_ error: Error) {
        let alert = NSAlert()
        alert.alertStyle = .critical
        alert.messageText = "Scry cannot start"
        alert.informativeText = error.displayMessage
        alert.addButton(withTitle: "Quit")
        NSApp.activate(ignoringOtherApps: true)
        alert.runModal()
        NSApp.terminate(nil)
    }

    func togglePanel() {
        if let panel, panel.isVisible {
            panel.orderOut(nil)
        } else {
            showPanel()
        }
    }

    func showSettings() {
        if settingsWindow == nil {
            let window = NSWindow(
                contentRect: NSRect(x: 0, y: 0, width: 760, height: 520),
                styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
                backing: .buffered,
                defer: false
            )
            window.title = "Scry Settings"
            window.toolbarStyle = .unified
            window.isReleasedWhenClosed = false
            window.titlebarAppearsTransparent = true
            window.isMovableByWindowBackground = true

            let hostingView = NSHostingView(rootView: SettingsView())
            hostingView.setFrameSize(hostingView.fittingSize)

            window.contentView = hostingView
            window.center()
            settingsWindow = window
        } else {
            NotificationCenter.default.post(name: .settingsDidShow, object: nil)
        }
        panel?.orderOut(nil)
        // Panel teardown re-activates the previous app this runloop turn; defer one turn.
        DispatchQueue.main.async { [self] in
            NSApp.activate(ignoringOtherApps: true)
            settingsWindow?.makeKeyAndOrderFront(nil)
            settingsWindow?.orderFrontRegardless()
        }
    }

    private func showPanel() {
        if panel == nil {
            let view = OverlayView(launcher: launcher) { [weak self] in
                self?.panel?.orderOut(nil)
            } onOpenSettings: { [weak self] in
                self?.showSettings()
            }
            let hosting = NSHostingView(rootView: view)
            hosting.sizingOptions = .preferredContentSize
            panel = ScryPanel(hosting: hosting)
        }
        launcher.refresh()
        panel?.show()
    }
}
