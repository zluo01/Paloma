//
//  Notification.Name+Panel.swift
//  Scry
//

import Foundation

extension Notification.Name {
    /// Posted on every hide path; the overlay resets itself for the next summon.
    static let panelDidHide = Notification.Name("panelDidHide")
    /// Posted when the cached settings window is shown again.
    static let settingsDidShow = Notification.Name("settingsDidShow")
}
