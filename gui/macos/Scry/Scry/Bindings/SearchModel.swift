//
//  SearchModel.swift
//  Scry
//
//

import Foundation
import Observation

enum SearchEvent {
    case action(index: Int)
    case subAction(index: Int)
    case chat
}

@MainActor
@Observable
final class SearchModel {
    private(set) var sections: [QueryResponse] = []
    var selection = 0
    var panelSelection: Int?

    @ObservationIgnored private var searchTask: Task<Void, Never>?

    func navigate(by delta: Int) {
        if let current = panelSelection, let item = selected?.item {
            let count = item.actions.count
            panelSelection = ((current + delta) % count + count) % count
            return
        }
        guard length > 0 else { return }
        selection = min(max(selection + delta, 0), length - 1)
    }

    func openPanel() {
        guard panelSelection == nil, let item = selected?.item, item.actions.count > 1 else { return }
        panelSelection = 0
    }

    @discardableResult
    func closePanel() -> Bool {
        guard panelSelection != nil else { return false }
        panelSelection = nil
        return true
    }

    func search(_ query: String) {
        clear()

        let input = query.trimmingCharacters(in: .whitespaces)
        guard !input.isEmpty else {
            return
        }

        searchTask = Task {
            // Query failures surface as empty results; core logs details.
            _ = await CoreClient.shared.withApp { app in
                let stream = try await app.search(input: input)
                while let event = await stream.next() {
                    if Task.isCancelled {
                        return
                    }
                    switch event {
                    case let .search(.append(response)):
                        // Rows without actions cannot be activated;
                        var response = response
                        response.items = response.items.filter { !$0.actions.isEmpty }
                        if !response.items.isEmpty {
                            let wasOnChatRow = chatRowSelected
                            sections.append(response)
                            if wasOnChatRow {
                                selection = length - 1
                            }
                        }
                    case .done:
                        return
                    default:
                        break
                    }
                }
            }
        }
    }

    func runAction(_ sectionId: ExtensionCapabilityId, action: Action) async -> Result<Void, Error> {
        await CoreClient.shared.withApp { app in
            _ = try await app.runSearchAction(extensionCapabilityId: sectionId, action: action)
        }
    }

    var itemCount: Int {
        sections.reduce(0) { $0 + $1.items.count }
    }

    var length: Int {
        itemCount == 0 ? 0 : itemCount + 1
    }

    /// Single source for row highlight and activation.
    var sectionBases: [Int] {
        var bases: [Int] = []
        var base = 0
        for section in sections {
            bases.append(base)
            base += section.items.count
        }
        return bases
    }

    /// Flat selection → (handler id, item).
    var selected: (sectionId: ExtensionCapabilityId, item: Item)? {
        for (base, section) in zip(sectionBases, sections) {
            let offset = selection - base
            if offset >= 0, offset < section.items.count {
                return (section.id, section.items[offset])
            }
        }
        return nil
    }

    /// What Return should run: panel highlight if open, else primary-or-first.
    func selectedAction() -> (sectionId: ExtensionCapabilityId, action: Action)? {
        guard let (sectionId, item) = selected else { return nil }
        if let panelSelection, panelSelection < item.actions.count {
            return (sectionId, item.actions[panelSelection])
        }
        guard let action = item.actions.first(where: \.primary) ?? item.actions.first else { return nil }
        return (sectionId, action)
    }

    var chatRowSelected: Bool {
        length > 0 && selection == length - 1
    }

    @discardableResult
    func clear() -> Bool {
        let hadResults = itemCount > 0
        searchTask?.cancel()
        sections = []
        selection = 0
        panelSelection = nil
        return hadResults
    }
}
