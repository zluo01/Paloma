//
//  SettingsView.swift
//  Paloma
//

import SwiftUI

enum SettingsPage: String, CaseIterable, Identifiable {
    case general = "General"
    case services = "Services"
    case plugins = "Plugins"
    case permissions = "Permissions"
    case shortcuts = "Shortcuts"

    var id: String {
        rawValue
    }

    var icon: String {
        switch self {
        case .general: "gearshape"
        case .services: "brain"
        case .plugins: "puzzlepiece.extension"
        case .permissions: "checkmark.shield"
        case .shortcuts: "keyboard"
        }
    }
}

struct SettingsView: View {
    @State private var services = ServiceModel()
    @State private var plugins = PluginModel()
    @State private var permissions = PermissionModel()
    @State private var page: SettingsPage = .general

    var body: some View {
        NavigationSplitView {
            List(SettingsPage.allCases, selection: $page) { page in
                Label(page.rawValue, systemImage: page.icon).tag(page)
            }
            .navigationSplitViewColumnWidth(180)
            .toolbar(removing: .sidebarToggle)
        } detail: {
            detail
                .navigationTitle(page.rawValue)
                // Keeps empty page toolbars so the title-bar height stays constant.
                .toolbar {
                    ToolbarItem {
                        Color.clear.frame(width: 0, height: 1)
                    }
                }
        }
        .onAppear(perform: refresh)
        .onChange(of: page) {
            refresh()
        }
        .onReceive(NotificationCenter.default.publisher(for: .settingsDidShow)) { _ in
            refresh()
        }
    }

    @ViewBuilder
    private var detail: some View {
        switch page {
        case .general:
            GeneralView()
        case .services:
            ServicesView(model: services)
        case .plugins:
            PluginsView(model: plugins)
        case .permissions:
            PermissionsView(model: permissions)
        case .shortcuts:
            ShortcutsView()
        }
    }

    private func refresh() {
        switch page {
        case .general:
            break
        case .services:
            services.refresh()
        case .plugins:
            plugins.refresh()
        case .permissions:
            permissions.refresh()
        case .shortcuts:
            break
        }
    }
}
