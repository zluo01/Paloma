//
//  ImageThumbnail.swift
//  Paloma
//

import AppKit

enum ImageThumbnail {
    static let maxAspect: CGFloat = 3

    static func size(for image: CGSize, height maxHeight: CGFloat) -> CGSize {
        guard image.width > 0, image.height > 0, maxHeight > 0 else { return .zero }
        let height = min(image.height, maxHeight)
        let width = min(image.width * height / image.height, height * maxAspect)
        return CGSize(width: width, height: height)
    }

    static func image(_ source: NSImage, height maxHeight: CGFloat) -> NSImage? {
        let size = size(for: source.size, height: maxHeight)
        guard size.width > 0, let cgImage = source.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
            return nil
        }
        let visibleWidth = source.size.height * size.width / size.height
        let scale = CGFloat(cgImage.width) / source.size.width
        let crop = CGRect(
            x: ((source.size.width - visibleWidth) / 2 * scale).rounded(.down),
            y: 0,
            width: (visibleWidth * scale).rounded(.up),
            height: CGFloat(cgImage.height)
        )
        guard let cropped = cgImage.cropping(to: crop) else { return nil }
        return NSImage(cgImage: cropped, size: size)
    }
}
