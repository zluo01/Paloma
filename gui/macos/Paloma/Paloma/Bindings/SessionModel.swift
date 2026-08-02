//
//  SessionModel.swift
//  Paloma
//
//

import Foundation
import Observation

@MainActor
@Observable
final class SessionModel {
    private(set) var sessions: [SessionListItem] = []
    /// Ids matched by the active search; nil while no search is active.
    private(set) var searchResult: Set<Uuid>?
    /// Keyboard cursor over `filtered`; nil until the user navigates.
    private(set) var selection: Int?

    private(set) var pendingDeletion: SessionListItem?

    @ObservationIgnored private var searchTask: Task<Void, Never>?

    var filtered: [SessionListItem] {
        guard let searchResult else { return sessions }
        return sessions.filter { searchResult.contains($0.sessionId) }
    }

    var selected: SessionListItem? {
        let visible = filtered
        guard let selection, visible.indices.contains(selection) else { return nil }
        return visible[selection]
    }

    func clear() {
        searchTask?.cancel()
        searchResult = nil
        selection = nil
        pendingDeletion = nil
    }

    func navigate(by delta: Int) {
        pendingDeletion = nil
        let sessions = filtered
        guard !sessions.isEmpty else { return }
        let anchor = selection ?? (delta > 0 ? -1 : sessions.count)
        selection = min(max(anchor + delta, 0), sessions.count - 1)
    }

    /// set pending delete item then update the selection index
    func pendingDelete(_ item: SessionListItem) {
        pendingDeletion = item
        if let index = filtered.firstIndex(where: { $0.sessionId == item.sessionId }) {
            selection = index
        }
    }

    func cancelDelete() {
        pendingDeletion = nil
    }

    /// Hands back the session to remove and resets the confirmation state.
    func confirmDelete() -> SessionListItem? {
        defer { pendingDeletion = nil }
        return pendingDeletion
    }

    func refresh() {
        CoreClient.shared.load({ try await $0.availableSessions() }, or: "failed to refresh sessions", category: "sessions") {
            self.sessions = $0
        }
    }

    func search(_ needle: String) {
        searchTask?.cancel()
        selection = nil
        pendingDeletion = nil

        let input = needle.trimmingCharacters(in: .whitespaces)
        guard !input.isEmpty else {
            searchResult = nil
            return
        }

        searchTask = Task {
            let result = await CoreClient.shared.withApp { app in
                try await app.searchSessions(needle: input)
            }
            guard !Task.isCancelled else { return }
            switch result {
            case let .success(ids):
                searchResult = Set(ids)
            case let .failure(error):
                logError(target: "sessions", message: "failed to search sessions: \(String(describing: error))")
            }
        }
    }

    func remove(_ item: SessionListItem) async -> Result<Void, Error> {
        await CoreClient.shared.withApp { app in
            try await app.removeSession(sessionId: item.sessionId)
            sessions.removeAll { $0.sessionId == item.sessionId }
            if let current = selection {
                selection = filtered.isEmpty ? nil : min(current, filtered.count - 1)
            }
        }
    }
}
