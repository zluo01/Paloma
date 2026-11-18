//
//  ChatStreamingPerformanceTests.swift
//  PalomaTests
//

import AppKit
@testable import Paloma
import SwiftUI
import XCTest

/// Time to stream one whole answer into the chat view, by answer size.
///
/// Not part of the normal test run; every case is skipped unless `PERFORMANCE_TESTS` is set:
///
///     TEST_RUNNER_PERFORMANCE_TESTS=1 xcodebuild test -scheme Paloma -destination 'platform=macOS' \
///         -only-testing:PalomaTests/ChatStreamingPerformanceTests
///
/// Results on an Apple M4, macOS 27.2 (the answer arrives 24 characters per update; ± is the standard deviation):
///
///     Benchmark            (blocks redone per update)   Updates   Cnt     Score     Error   Units
///     testStreaming2KB                   all (before)        84     3     0.241 ±   0.011    s/op
///     testStreaming2KB                   changed only        84     3     0.241 ±   0.045    s/op
///     testStreaming4KB                   all (before)       167     3     0.600 ±   0.029    s/op
///     testStreaming4KB                   changed only       167     3     0.517 ±   0.012    s/op
///     testStreaming8KB                   all (before)       334     3     1.427 ±   0.016    s/op
///     testStreaming8KB                   changed only       334     3     1.106 ±   0.010    s/op
///     testStreaming16KB                  all (before)       667     3     4.349 ±   0.020    s/op
///     testStreaming16KB                  changed only       667     3     2.560 ±   0.036    s/op
///     testStreaming32KB                  all (before)      1334     3    14.765 ±   0.019    s/op
///     testStreaming32KB                  changed only      1334     3     7.186 ±   0.085    s/op
final class ChatStreamingPerformanceTests: XCTestCase {
    override func setUpWithError() throws {
        try XCTSkipUnless(
            ProcessInfo.processInfo.environment["PERFORMANCE_TESTS"] != nil,
            "performance tests only run when PERFORMANCE_TESTS is set"
        )
    }

    @MainActor
    func testStreaming2KB() {
        measureStreaming(kilobytes: 2)
    }

    @MainActor
    func testStreaming4KB() {
        measureStreaming(kilobytes: 4)
    }

    @MainActor
    func testStreaming8KB() {
        measureStreaming(kilobytes: 8)
    }

    @MainActor
    func testStreaming16KB() {
        measureStreaming(kilobytes: 16)
    }

    @MainActor
    func testStreaming32KB() {
        measureStreaming(kilobytes: 32)
    }

    @MainActor
    private func measureStreaming(kilobytes: Int) {
        let answer = Self.answer(kilobytes: kilobytes)
        let options = XCTMeasureOptions()
        options.iterationCount = 3
        measure(metrics: [XCTClockMetric()], options: options) {
            Self.stream(answer)
        }
    }

    /// Feeds `answer` into a real chat view a few characters at a time, updating the view after each piece.
    @MainActor
    private static func stream(_ answer: String) {
        let model = ChatModel()
        let hosting = NSHostingView(rootView: ChatView(model: model).frame(width: 640))
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 640, height: 400), styleMask: [.borderless], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.contentView = hosting
        defer {
            window.contentView = nil
            window.close()
        }

        let backend = ProviderBackendId(providerId: "Anthropic", backendId: "claude")
        let characters = Array(answer)
        for offset in stride(from: 0, to: characters.count, by: 24) {
            let piece = String(characters[offset ..< min(offset + 24, characters.count)])
            model.render(.chat(event: .textDelta(providerBackendId: backend, text: piece)))
            hosting.layoutSubtreeIfNeeded()
            RunLoop.main.run(until: Date().addingTimeInterval(0.001))
            hosting.layoutSubtreeIfNeeded()
        }
    }

    private static func answer(kilobytes: Int) -> String {
        let length = kilobytes * 1000
        let repeats = length / section.count + 1
        return String(Array(repeating: section, count: repeats).joined(separator: "\n\n").prefix(length))
    }

    /// Lorem ipsum filler laid out with every markdown block the chat renders.
    private static let section = """
    ## Lorem ipsum dolor sit amet

    Lorem ipsum dolor sit amet, **consectetur adipiscing** elit, sed do `eiusmod tempor` incididunt ut labore
    et dolore magna aliqua. Ut enim ad [minim veniam](https://example.com), quis nostrud exercitation ullamco
    laboris nisi ut aliquip ex ea commodo consequat.

    - Duis aute irure dolor in *reprehenderit*
    - in voluptate velit esse `cillum` dolore
      - eu fugiat nulla pariatur
    - Excepteur sint occaecat cupidatat non proident

    ```swift
    func lorem(ipsum: Int) -> Int {
        let dolor = ipsum * 2
        return dolor + 1
    }
    ```

    | Lorem | Ipsum | Dolor |
    | --- | --- | --- |
    | sit | amet | consectetur |
    | adipiscing | elit | sed |

    > Sunt in culpa qui officia deserunt mollit anim id est laborum.
    """
}
