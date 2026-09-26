//
//  QueryModel.swift
//  Paloma
//
//

import AppKit
import Observation
import UniformTypeIdentifiers

@MainActor
@Observable
final class QueryModel {
    private(set) var content = NSAttributedString()
    @ObservationIgnored weak var textView: ComposerTextView?

    var text: String {
        content.string.replacingOccurrences(of: "\u{FFFC}", with: "")
    }

    var isEmpty: Bool {
        content.length == 0
    }

    /// The mode only flips to chat for prompts that survive submitChat's trimming.
    var hasQuery: Bool {
        !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    func prompt() -> (text: String, attachments: [UserPromptAttachment]) {
        var text = ""
        var attachments: [UserPromptAttachment] = []
        content.enumerateAttribute(.attachment, in: NSRange(location: 0, length: content.length)) { value, range, _ in
            guard let attachment = value as? NSTextAttachment else {
                text += content.attributedSubstring(from: range).string
                return
            }
            guard let data = attachment.contents,
                  let mediaType = attachment.fileType.flatMap(UTType.init)?.preferredMIMEType
            else {
                return
            }
            let id = UInt32(attachments.count + 1)
            text += "[Image #\(id)]"
            attachments.append(.image(id: id, mediaType: mediaType, data: data))
        }
        let prompt = text.replacingOccurrences(of: "\u{FFFC}", with: "").trimmingCharacters(in: .whitespacesAndNewlines)
        return (prompt, attachments)
    }

    func sync() {
        guard let textView else { return }
        content = NSAttributedString(attributedString: textView.attributedString())
    }

    func clear() {
        textView?.clear()
        content = NSAttributedString()
    }
}
