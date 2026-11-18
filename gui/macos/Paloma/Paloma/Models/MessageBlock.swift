//
//  MessageBlock.swift
//  Paloma
//
//

/// Inline-only AttributedString cannot express blocks, so lines split into blocks first.
/// Equatable so SwiftUI can skip the blocks that did not change while an answer streams.
struct MessageBlock: Identifiable, Equatable {
    enum Kind: Equatable {
        case paragraph(String)
        case heading(Int, String)
        case code(String)
        case table(header: [String], rows: [[String]])
        case quote(String)
        case list([ListItem])
    }

    struct ListItem: Equatable {
        let indent: Int
        let marker: String
        let text: String
    }

    let id: Int
    let kind: Kind
}
