//
//  QueryView.swift
//  Scry
//
//

import SwiftUI

struct QueryView: View {
    @Binding var query: String
    let mode: OverlayMode
    let onSearch: (String) -> Void
    @FocusState private var focused: Bool

    private var placeholder: String {
        switch mode {
        case .search: "Search, or ask anything…"
        case .chat: "Reply…"
        case .session: "Search sessions…"
        }
    }

    private var icon: String {
        switch mode {
        case .search: "magnifyingglass"
        case .chat: "sparkles"
        case .session: "clock.arrow.circlepath"
        }
    }

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 12) {
            Image(systemName: icon)
                .font(.system(size: 20, weight: .light))
                .foregroundStyle(.secondary)
            TextField(placeholder, text: $query)
                .textFieldStyle(.plain)
                .font(.system(size: 22, weight: .light))
                .focused($focused)
                .task(id: query) {
                    if !query.isEmpty {
                        guard await (try? Task.sleep(for: .milliseconds(150))) != nil else { return }
                    }
                    onSearch(query)
                }
        }
        .padding(.horizontal, 18)
        .frame(height: 58)
        // First show: this body runs during layout, before the panel is
        // key, and a focus request on a non-key window is refused — defer
        // it one runloop turn. The listener covers every later summon.
        .onAppear {
            DispatchQueue.main.async {
                focused = true
            }
        }
        .onReceive(
            NotificationCenter.default.publisher(for: NSWindow.didBecomeKeyNotification)
        ) { note in
            if (note.object as? NSWindow) is ScryPanel {
                focused = true
            }
        }
    }
}
