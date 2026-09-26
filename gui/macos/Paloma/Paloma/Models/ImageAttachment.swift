//
//  ImageAttachment.swift
//  Paloma
//
//

import AppKit
import UniformTypeIdentifiers

final class ImageAttachment: NSTextAttachment {
    let thumbnail: NSImage

    init(data: Data, type: UTType, thumbnail: NSImage) {
        self.thumbnail = thumbnail
        super.init(data: data, ofType: type.identifier)
    }

    required init?(coder _: NSCoder) {
        nil
    }

    override func image(forBounds _: CGRect, textContainer _: NSTextContainer?, characterIndex _: Int) -> NSImage? {
        thumbnail
    }
}
