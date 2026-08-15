//
//  SessionListItem+DisplayTitle.swift
//  Paloma
//

extension SessionListItem {
    var displayTitle: String {
        title.isEmpty ? "Untitled session" : title
    }
}
