//
//  PalomaMacApp.swift
//  Paloma
//

import SwiftUI

/// Named to avoid colliding with the FFI-generated `PalomaApp` core handle.
@main
struct PalomaMacApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate

    var body: some Scene {
        MenuBarExtra("Paloma", systemImage: "sparkle.magnifyingglass") {
            Button("Toggle Paloma") {
                appDelegate.togglePanel()
            }
            Button("Settings…") {
                appDelegate.showSettings()
            }
            Divider()
            Button("Quit Paloma") {
                NSApp.terminate(nil)
            }
        }
    }
}
