//
//  IconResolver.swift
//  Scry
//
//  Core hands the frontend icon identifiers, not pixels.
//

import AppKit
import SwiftUI

enum ResolvedIcon {
    case system(String)
    case image(NSImage)
}

enum IconResolver {
    private static let cache = NSCache<NSString, NSImage>()

    /// Freedesktop icon names used by core capabilities.
    private static let symbols: [String: String] = [
        "edit-paste": "doc.on.clipboard",
        "accessories-calculator": "plus.forwardslash.minus",
        "folder": "folder",
    ]

    static func resolve(_ icon: IconRef?) -> ResolvedIcon {
        switch icon {
        case let .path(path):
            if let cached = cache.object(forKey: path as NSString) {
                return .image(cached)
            }
            let image = NSWorkspace.shared.icon(forFile: path)
            cache.setObject(image, forKey: path as NSString)
            return .image(image)
        case let .name(name):
            return .system(symbols[name] ?? mimeSymbol(name))
        case let .embedded(_, data):
            if let image = NSImage(data: Data(data)) {
                return .image(image)
            }
            return .system("questionmark.square.dashed")
        case nil:
            return .system("magnifyingglass")
        }
    }

    /// FileSearch encodes mime types as names like "image-png".
    private static func mimeSymbol(_ name: String) -> String {
        switch name.split(separator: "-").first {
        case "image": "photo"
        case "video": "film"
        case "audio": "music.note"
        case "text": "doc.text"
        case "application": "doc"
        default: "doc"
        }
    }
}

struct IconView: View {
    private let resolved: ResolvedIcon

    init(icon: IconRef?) {
        resolved = IconResolver.resolve(icon)
    }

    init(systemName: String) {
        resolved = .system(systemName)
    }

    var body: some View {
        switch resolved {
        case let .system(name):
            Image(systemName: name)
                .font(.system(size: 16))
                .foregroundStyle(.secondary)
                .frame(width: 28, height: 28)
        case let .image(image):
            Image(nsImage: image)
                .resizable()
                .frame(width: 28, height: 28)
        }
    }
}
