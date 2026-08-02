//
//  SessionListItem+DisplayTitle.swift
//  Paloma
//

import Foundation

extension SessionListItem {
    var displayTitle: String {
        title.isEmpty ? "Untitled session" : title
    }
}
