//
//  IconResolver.swift
//  Paloma
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
    private static let cache: NSCache<NSString, NSImage> = {
        let cache = NSCache<NSString, NSImage>()
        cache.countLimit = 512
        return cache
    }()

    static func resolve(_ icon: Icon?) -> ResolvedIcon {
        switch icon {
        case let .path(path):
            if let cached = cache.object(forKey: path as NSString) {
                return .image(cached)
            }
            let image = NSWorkspace.shared.icon(forFile: path)
            cache.setObject(image, forKey: path as NSString)
            return .image(image)
        case let .name(name):
            guard NSImage(systemSymbolName: name, accessibilityDescription: nil) != nil else {
                return .system("questionmark.square.dashed")
            }
            return .system(name)
        case let .embedded(data):
            if let image = NSImage(data: Data(data)) {
                return .image(image)
            }
            return .system("questionmark.square.dashed")
        case nil:
            return .system("magnifyingglass")
        }
    }
}

struct IconView: View {
    private let resolved: ResolvedIcon

    init(icon: Icon?) {
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
