//
//  QueryView.swift
//  Paloma
//
//

import AppKit
import SwiftUI
import UniformTypeIdentifiers

struct QueryView: View {
    let query: QueryModel
    let mode: OverlayMode
    let onSearch: (String) -> Void
    let onSubmit: () -> Void
    let onNavigate: (Int) -> Void
    let onEscape: () -> Void

    /// indicator to tell if current input through IME
    @State private var composing = false

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
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: icon)
                .font(.system(size: 20, weight: .light))
                .foregroundStyle(.secondary)
                .padding(.top, 2)
            ZStack(alignment: .topLeading) {
                if query.isEmpty, !composing {
                    Text(placeholder)
                        .font(.system(size: 22, weight: .light))
                        .foregroundStyle(Color(nsColor: .placeholderTextColor))
                }
                InputView(
                    query: query,
                    composing: $composing,
                    onSubmit: onSubmit,
                    onNavigate: onNavigate,
                    onEscape: onEscape
                )
            }
        }
        .padding(.horizontal, 18)
        .padding(.vertical, 16)
        .task(id: query.text) {
            if !query.text.isEmpty {
                guard await (try? Task.sleep(for: .milliseconds(150))) != nil else { return }
            }
            onSearch(query.text)
        }
    }
}
