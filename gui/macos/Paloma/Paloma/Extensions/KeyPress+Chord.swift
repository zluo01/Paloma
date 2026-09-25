//
//  KeyPress+Chord.swift
//  Paloma
//

import SwiftUI

extension KeyPress {
    func chord(_ required: EventModifiers = []) -> Bool {
        guard (NSApp.keyWindow?.firstResponder as? NSTextView)?.hasMarkedText() != true else { return false }
        return modifiers.intersection([.command, .shift, .option, .control]) == required
    }
}
