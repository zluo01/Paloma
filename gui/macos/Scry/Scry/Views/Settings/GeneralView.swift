//
//  GeneralView.swift
//  Scry
//

import KeyboardShortcuts
import ServiceManagement
import SwiftUI

struct GeneralView: View {
    @State private var launchAtLogin = false
    @State private var operationError: OperationError?

    var body: some View {
        Form {
            Section("Startup") {
                Toggle("Launch at login", isOn: $launchAtLogin)
                    .onChange(of: launchAtLogin) {
                        let actual = SMAppService.mainApp.status == .enabled
                        // The failure revert re-enters here; matching reality means nothing to do.
                        guard launchAtLogin != actual else { return }
                        do {
                            if launchAtLogin {
                                try SMAppService.mainApp.register()
                            } else {
                                try SMAppService.mainApp.unregister()
                            }
                        } catch {
                            operationError = OperationError(
                                title: "Failed to Update Login Item",
                                message: error.localizedDescription
                            )
                            launchAtLogin = actual
                        }
                    }
            }
            Section("Shortcut") {
                KeyboardShortcuts.Recorder("Toggle Scry", name: .toggleScry)
            }
        }
        .formStyle(.grouped)
        .onAppear {
            launchAtLogin = SMAppService.mainApp.status == .enabled
        }
        .onReceive(NotificationCenter.default.publisher(for: .settingsDidShow)) { _ in
            launchAtLogin = SMAppService.mainApp.status == .enabled
        }
        .operationErrorAlert($operationError)
    }
}
