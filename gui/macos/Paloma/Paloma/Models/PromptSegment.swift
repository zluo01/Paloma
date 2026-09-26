//
//  PromptSegment.swift
//  Paloma
//
//

import Foundation

enum PromptSegment: Equatable {
    case text(String)
    case image(UInt32)

    private static let placeholderPrefix = "[Image #"

    static func placeholder(_ id: UInt32) -> String {
        "\(placeholderPrefix)\(id)]"
    }

    static func split(_ text: String) -> [PromptSegment] {
        var segments: [PromptSegment] = []
        var rest = Substring(text)
        while let prefix = rest.range(of: placeholderPrefix) {
            let afterPrefix = rest[prefix.upperBound...]
            guard let close = afterPrefix.firstIndex(of: "]") else { break }
            guard let id = UInt32(afterPrefix[..<close]) else {
                segments.append(.text(String(rest[..<prefix.upperBound])))
                rest = afterPrefix
                continue
            }
            if prefix.lowerBound > rest.startIndex {
                segments.append(.text(String(rest[..<prefix.lowerBound])))
            }
            segments.append(.image(id))
            rest = afterPrefix[afterPrefix.index(after: close)...]
        }
        if !rest.isEmpty {
            segments.append(.text(String(rest)))
        }
        return segments
    }
}
