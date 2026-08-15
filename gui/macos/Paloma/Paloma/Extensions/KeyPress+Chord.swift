//
//  KeyPress+Chord.swift
//  Paloma
//

import SwiftUI

extension KeyPress {
    func chord(_ required: EventModifiers = []) -> Bool {
        modifiers.intersection([.command, .shift, .option, .control]) == required
    }
}
