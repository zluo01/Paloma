//
//  ImageThumbnailTests.swift
//  PalomaTests
//

import AppKit
@testable import Paloma
import Testing

@MainActor
struct ImageThumbnailTests {
    @Test func givenSmallImageWhenSizingShouldKeepItsSize() {
        #expect(ImageThumbnail.size(for: CGSize(width: 20, height: 10), height: 26) == CGSize(width: 20, height: 10))
    }

    @Test func givenTallImageWhenSizingShouldScaleToTheHeightCap() {
        #expect(ImageThumbnail.size(for: CGSize(width: 400, height: 200), height: 26) == CGSize(width: 52, height: 26))
    }

    @Test func givenWideImageWhenSizingShouldCapTheAspectAtThreeToOne() {
        #expect(ImageThumbnail.size(for: CGSize(width: 400, height: 40), height: 26) == CGSize(width: 78, height: 26))
    }

    @Test func givenEmptyImageWhenSizingShouldReturnZero() {
        #expect(ImageThumbnail.size(for: .zero, height: 26) == .zero)
    }

    @Test func givenWideImageWhenRenderingShouldMatchTheCappedSize() throws {
        let thumbnail = try #require(ImageThumbnail.image(Self.halves(width: 400, height: 40), height: 26))
        #expect(thumbnail.size == CGSize(width: 78, height: 26))
    }

    @Test func givenWideImageWhenRenderingShouldKeepTheCentreAndDropTheEdges() throws {
        let thumbnail = try #require(ImageThumbnail.image(Self.halves(width: 400, height: 40), height: 26))
        let cropped = try #require(thumbnail.cgImage(forProposedRect: nil, context: nil, hints: nil))
        #expect(cropped.width == 120)
        #expect(cropped.height == 40)
        #expect(Self.isRed(cropped, x: 10, y: 20))
        #expect(Self.isBlue(cropped, x: 110, y: 20))
    }

    @Test func givenImageWithinTheAspectCapWhenRenderingShouldNotCrop() throws {
        let thumbnail = try #require(ImageThumbnail.image(Self.halves(width: 60, height: 40), height: 26))
        let cropped = try #require(thumbnail.cgImage(forProposedRect: nil, context: nil, hints: nil))
        #expect(cropped.width == 60)
        #expect(cropped.height == 40)
        #expect(thumbnail.size == CGSize(width: 39, height: 26))
    }

    // MARK: - Helpers

    private static func halves(width: Int, height: Int) -> NSImage {
        let context = CGContext(
            data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0,
            space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
        )!
        context.setFillColor(CGColor(red: 1, green: 0, blue: 0, alpha: 1))
        context.fill(CGRect(x: 0, y: 0, width: width / 2, height: height))
        context.setFillColor(CGColor(red: 0, green: 0, blue: 1, alpha: 1))
        context.fill(CGRect(x: width / 2, y: 0, width: width / 2, height: height))
        return NSImage(cgImage: context.makeImage()!, size: NSSize(width: width, height: height))
    }

    private static func pixel(_ image: CGImage, x: Int, y: Int) -> (red: UInt8, blue: UInt8)? {
        guard let data = image.dataProvider?.data as Data?, let bitsPerPixel = Optional(image.bitsPerPixel), bitsPerPixel == 32 else { return nil }
        let offset = y * image.bytesPerRow + x * 4
        let alphaFirst = image.alphaInfo == .premultipliedFirst || image.alphaInfo == .first || image.alphaInfo == .noneSkipFirst
        let littleEndian = image.byteOrderInfo == .order32Little
        let bytes = [data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]
        switch (alphaFirst, littleEndian) {
        case (true, true): return (red: bytes[2], blue: bytes[0]) // BGRA
        case (true, false): return (red: bytes[1], blue: bytes[3]) // ARGB
        case (false, true): return (red: bytes[2], blue: bytes[0]) // ABGR
        case (false, false): return (red: bytes[0], blue: bytes[2]) // RGBA
        }
    }

    private static func isRed(_ image: CGImage, x: Int, y: Int) -> Bool {
        guard let p = pixel(image, x: x, y: y) else { return false }
        return p.red > 150 && p.blue < 100
    }

    private static func isBlue(_ image: CGImage, x: Int, y: Int) -> Bool {
        guard let p = pixel(image, x: x, y: y) else { return false }
        return p.blue > 150 && p.red < 100
    }
}
