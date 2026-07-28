//
//  OverlayView.swift
//  Scry
//
//  The panel window follows this view's size.
//

import SwiftUI

enum OverlayMode {
    case search
    case chat
    case session
}

struct OverlayView: View {
    @State private var mode: OverlayMode = .search
    @State private var query: String = ""
    @State private var operationError: OperationError?

    @State private var searches = SearchModel()
    @State private var chats = ChatModel()
    @State private var sessions = SessionModel()

    let launcher: LauncherModel
    var onHide: () -> Void
    var onOpenSettings: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            QueryView(query: $query, mode: mode, onSearch: dispatchQuery)
                .onSubmit {
                    handleSubmit()
                }
                .onKeyPress(.upArrow) {
                    handleNavigate(-1)
                    return .handled
                }
                .onKeyPress(keys: [.downArrow]) { press in
                    if press.modifiers.contains(.shift) {
                        openSession()
                    } else {
                        handleNavigate(1)
                    }
                    return .handled
                }
                .onKeyPress(.escape) {
                    handleEscape()
                    return .handled
                }
                .onKeyPress(keys: [.return]) { press in
                    // ⌃⏎ is taken by the system context-menu shortcut, so ⌘⏎.
                    guard press.modifiers.contains(.command) else { return .ignored }
                    if mode == .search {
                        searches.openPanel()
                        return .handled
                    }
                    return .ignored
                }
                .onKeyPress(keys: ["c"]) { press in
                    guard press.modifiers.contains(.control) else { return .ignored }
                    if mode == .chat {
                        chats.interrupt()
                        return .handled
                    }
                    return .ignored
                }
                .onKeyPress(.deleteForward) {
                    guard mode == .session, let item = sessions.selected else { return .ignored }
                    removeSession(item)
                    return .handled
                }
                .onKeyPress(phases: .down) { press in
                    // Text edits are blocked while the action panel is open.
                    guard mode == .search, searches.panelSelection != nil else {
                        return .ignored
                    }
                    switch press.key {
                    case .return, .escape, .upArrow, .downArrow:
                        return .ignored
                    default:
                        return .handled
                    }
                }

            content
            if let operationError {
                ErrorBannerView(error: operationError) {
                    self.operationError = nil
                }
            }
            Divider()
            FooterView(
                model: launcher,
                mode: mode,
                onOpenSettings: onOpenSettings,
                onOpenSession: openSession,
                onSelectModel: selectModel
            )
        }
        .frame(width: 640)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 14))
        .onReceive(NotificationCenter.default.publisher(for: .panelDidHide)) { _ in
            operationError = nil
        }
        .onChange(of: mode) { previous, current in
            // Chat submissions capture the prompt before the transition clears the field.
            query.removeAll()
            switch previous {
            case .search:
                searches.clear()
            case .session:
                sessions.clear()
            case .chat:
                chats.clear()
            }
            if current == .session {
                sessions.refresh()
            }
        }
    }

    @ViewBuilder
    private var content: some View {
        switch mode {
        case .search:
            SearchView(
                query: query,
                sections: searches.sections,
                bases: searches.sectionBases,
                selection: searches.selection,
                panelSelection: searches.panelSelection,
                selectedItem: searches.selected?.item,
                chatRowSelected: searches.chatRowSelected,
                onEvent: handleSearchEvent
            )
        case .chat:
            ChatView(model: chats)
        case .session:
            SessionsView(
                sessions: sessions.filtered,
                selection: sessions.selection,
                onRestore: restoreSession,
                onDelete: removeSession
            )
        }
    }

    private func restoreSession(_ item: SessionListItem) {
        chats.restore(sessionId: item.sessionId)
        mode = .chat
    }

    private func removeSession(_ item: SessionListItem) {
        OperationError.run("Failed to Remove Session", into: $operationError) {
            await sessions.remove(item)
        }
    }

    private func handleSearchEvent(_ event: SearchEvent) {
        switch event {
        case let .action(index):
            searches.selection = index
            searches.closePanel()
            runSelected()
        case let .subAction(index):
            searches.panelSelection = index
            runSelected()
        case .chat:
            startChat()
        }
    }

    /// The mode only flips to chat for prompts that survive submitChat's trimming.
    private var hasPrompt: Bool {
        !query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    private func startChat() {
        guard hasPrompt else { return }
        chats.submitChat(query)
        mode = .chat
    }

    private func runSelected() {
        guard let (sectionId, action) = searches.selectedAction() else { return }
        OperationError.run("Failed to Run Action", into: $operationError) {
            await searches.runAction(sectionId, action: action)
        } onSuccess: {
            onHide()
        }
    }

    private func dispatchQuery(_ input: String) {
        switch mode {
        case .search:
            searches.search(input)
        case .session:
            sessions.search(input)
        case .chat:
            break
        }
    }

    private func openSession() {
        if mode == .session {
            mode = .search
        } else {
            mode = .session
        }
    }

    private func selectModel(_ providerBackendId: ProviderBackendId, _ model: String, _ effort: String) {
        OperationError.run("Failed to Set Model", into: $operationError) {
            await CoreClient.shared.setModelPreference(providerBackendId, model: model, effort: effort, setDefault: true)
        } onSuccess: {
            launcher.refresh()
        }
    }

    private func handleNavigate(_ delta: Int) {
        switch mode {
        case .search:
            searches.navigate(by: delta)
        case .session:
            sessions.navigate(by: delta)
        case .chat:
            chats.navigate(by: delta)
        }
    }

    private func handleSubmit() {
        switch mode {
        case .search:
            if searches.sections.isEmpty || searches.chatRowSelected {
                startChat()
            } else {
                runSelected()
            }
        case .chat:
            if !chats.decideSelected(), chats.chatStatus != .streaming {
                chats.submitChat(query)
                query.removeAll()
            }
        case .session:
            if let item = sessions.selected {
                restoreSession(item)
            }
        }
    }

    private func handleEscape() {
        switch mode {
        case .search:
            // An open action panel swallows the first escape.
            if searches.closePanel() {
                return
            }
            query.removeAll()
            searches.clear()
            onHide()
        case .chat, .session:
            mode = .search
        }
    }
}
