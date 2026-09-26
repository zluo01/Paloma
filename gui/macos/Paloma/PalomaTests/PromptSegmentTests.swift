//
//  PromptSegmentTests.swift
//  PalomaTests
//

@testable import Paloma
import Testing

struct PromptSegmentTests {
    @Test func givenPlaceholdersBetweenTextWhenSplittingShouldKeepThemInOrder() {
        #expect(PromptSegment.split("compare [Image #1] with [Image #2], ok?") == [
            .text("compare "), .image(1), .text(" with "), .image(2), .text(", ok?"),
        ])
    }

    @Test func givenOnlyAPlaceholderWhenSplittingShouldReturnJustTheImage() {
        #expect(PromptSegment.split("[Image #1]") == [.image(1)])
    }

    @Test func givenPlaceholdersAtBothEdgesWhenSplittingShouldNotAddEmptyText() {
        #expect(PromptSegment.split("[Image #1] hi [Image #2]") == [.image(1), .text(" hi "), .image(2)])
    }

    @Test func givenMalformedPlaceholdersWhenSplittingShouldKeepThemAsText() {
        #expect(PromptSegment.split("see [Image #x] and [Image #") == [
            .text("see [Image #"), .text("x] and [Image #"),
        ])
    }

    @Test func givenPlainTextWhenSplittingShouldReturnOneTextSegment() {
        #expect(PromptSegment.split("plain text") == [.text("plain text")])
    }

    @Test func givenAnIdWhenMakingAPlaceholderShouldSplitBackToTheSameImage() {
        #expect(PromptSegment.placeholder(3) == "[Image #3]")
        #expect(PromptSegment.split(PromptSegment.placeholder(3)) == [.image(3)])
    }
}
