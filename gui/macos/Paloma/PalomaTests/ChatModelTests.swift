//
//  ChatModelTests.swift
//  PalomaTests
//

import AppKit
@testable import Paloma
import Testing
import UniformTypeIdentifiers

@MainActor
struct ChatModelTests {
    private let chats = ChatModel()

    @Test func givenUserPromptWithImageWhenRenderingShouldKeepTextAndImageById() throws {
        let png = try imageData(.png, width: 4, height: 2)

        chats.render(.chat(event: .userPrompt(
            text: "look [Image #2]",
            attachments: [.image(id: 2, mediaType: "image/png", data: png)]
        )))

        guard case let .user(_, text, images) = chats.transcript.last else {
            Issue.record("expected a user section")
            return
        }
        #expect(text == "look [Image #2]")
        #expect(images.keys.sorted() == [2])
        #expect(images[2]?.size == CGSize(width: 4, height: 2))
    }

    @Test func givenUserPromptWithUndecodableImageWhenRenderingShouldSkipIt() {
        chats.render(.chat(event: .userPrompt(
            text: "[Image #1]",
            attachments: [.image(id: 1, mediaType: "image/png", data: Data("not an image".utf8))]
        )))

        guard case let .user(_, _, images) = chats.transcript.last else {
            Issue.record("expected a user section")
            return
        }
        #expect(images.isEmpty)
    }
}
