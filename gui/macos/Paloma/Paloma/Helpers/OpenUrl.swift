//
//  OpenUrl.swift
//  Paloma
//
//  URL opening is frontend policy; the core never opens a browser itself.
//

import AppKit

func openUrl(_ url: String) {
    if let parsed = URL(string: url) {
        NSWorkspace.shared.open(parsed)
    }
}
