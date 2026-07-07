//
//  ScryMacApp.swift
//  Scry
//

import SwiftUI

/// Named to avoid colliding with the FFI-generated `ScryApp` core handle.
@main
struct ScryMacApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate

    var body: some Scene {
        MenuBarExtra("Scry", systemImage: "sparkle.magnifyingglass") {
            Button("Toggle Scry") {
                appDelegate.togglePanel()
            }
            Button("Settings…") {
                appDelegate.showSettings()
            }
            Divider()
            Button("Quit Scry") {
                NSApp.terminate(nil)
            }
        }
    }
}
