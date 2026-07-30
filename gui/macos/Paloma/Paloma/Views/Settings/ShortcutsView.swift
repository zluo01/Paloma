//
//  ShortcutsView.swift
//  Paloma
//

import SwiftUI

struct ShortcutsView: View {
    var body: some View {
        Form {
            Section("Navigation") {
                LabeledContent("Move up", value: "↑")
                LabeledContent("Move down", value: "↓")
                LabeledContent("Back / dismiss", value: "esc")
            }
            Section("Search") {
                LabeledContent("Open / run action", value: "⏎")
                LabeledContent("Show actions", value: "⌘⏎")
                LabeledContent("Open sessions", value: "⇧↓")
            }
            Section("Chat") {
                LabeledContent("Send message", value: "⏎")
                LabeledContent("Stop generating", value: "⌃C")
                LabeledContent("Open sessions", value: "⇧↓")
            }
            Section("Sessions") {
                LabeledContent("Restore session", value: "⏎")
                LabeledContent("Delete session", value: "⌦")
            }
        }
        .formStyle(.grouped)
    }
}
