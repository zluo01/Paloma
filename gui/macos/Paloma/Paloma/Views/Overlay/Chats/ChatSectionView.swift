//
//  ChatSectionView.swift
//  Paloma
//
//

import SwiftUI

struct ChatSectionView: View {
    private static let inlineImageHeight: CGFloat = 16
    private static let displayImageHeight: CGFloat = 120

    let section: ChatSection
    let model: ChatModel

    var body: some View {
        switch section {
        case let .user(_, text, images):
            let segments = PromptSegment.split(text)
            HStack {
                Spacer(minLength: 60)
                VStack(alignment: .leading, spacing: 6) {
                    Self.promptText(segments, images: images)
                        .textSelection(.enabled)
                    let shown = Self.shownImages(segments, images: images)
                    if !shown.isEmpty {
                        WrapLayout {
                            ForEach(shown.indices, id: \.self) { index in
                                let image = shown[index]
                                let size = ImageThumbnail.size(for: image.size, height: Self.displayImageHeight)
                                Image(nsImage: image)
                                    .resizable()
                                    .scaledToFill()
                                    .frame(width: size.width, height: size.height)
                                    .clipped()
                            }
                        }
                    }
                }
                .font(.system(size: 13))
                .padding(.horizontal, 12)
                .padding(.vertical, 7)
                .background(.selection, in: RoundedRectangle(cornerRadius: 12))
            }
        case let .assistant(_, providerBackendId, text):
            VStack(alignment: .leading, spacing: 3) {
                Text(providerBackendId.label)
                    .font(.caption2.weight(.semibold))
                    .foregroundStyle(.secondary)
                MessageText(text: text)
            }
        case let .reasoning(_, text):
            ReasoningView(text: text)
        case let .tool(tool):
            ToolCallView(tool: tool, model: model)
        }
    }

    private static func promptText(_ segments: [PromptSegment], images: [UInt32: NSImage]) -> Text {
        segments.reduce(Text(verbatim: "")) { result, segment in
            switch segment {
            case let .text(run):
                Text("\(result)\(Text(verbatim: run))")
            case let .image(id):
                if let image = images[id] {
                    Text("\(result)\(Image(nsImage: inlineImage(image)))")
                } else {
                    Text("\(result)\(Text(verbatim: PromptSegment.placeholder(id)))")
                }
            }
        }
    }

    private static func shownImages(_ segments: [PromptSegment], images: [UInt32: NSImage]) -> [NSImage] {
        segments.compactMap { segment in
            guard case let .image(id) = segment else { return nil }
            return images[id]
        }
    }

    private static func inlineImage(_ image: NSImage) -> NSImage {
        let height = min(image.size.height, inlineImageHeight)
        let size = CGSize(width: image.size.width * height / max(image.size.height, 1), height: height)
        return NSImage(size: size, flipped: false) { rect in
            image.draw(in: rect)
            return true
        }
    }
}

private struct MessageText: View {
    let text: String

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            ForEach(blocks) { block in
                MessageBlockView(block: block)
            }
        }
    }

    private var blocks: [MessageBlock] {
        var result: [MessageBlock] = []
        var paragraph: [Substring] = []
        var code: [Substring] = []
        var tableLines: [Substring] = []
        var quoteLines: [String] = []
        var listItems: [MessageBlock.ListItem] = []
        var inCode = false

        func append(_ kind: MessageBlock.Kind) {
            result.append(MessageBlock(id: result.count, kind: kind))
        }

        func flushParagraph() {
            let content = paragraph.joined(separator: "\n")
            paragraph = []
            if !content.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                append(.paragraph(content))
            }
        }

        func flushTable() {
            guard !tableLines.isEmpty else { return }
            var rows = tableLines.map(cells)
            tableLines = []
            var header: [String] = []
            if rows.count >= 2, isSeparator(rows[1]) {
                header = rows.removeFirst()
                rows.removeFirst()
            }
            append(.table(header: header, rows: rows))
        }

        func flushQuote() {
            guard !quoteLines.isEmpty else { return }
            append(.quote(quoteLines.joined(separator: "\n")))
            quoteLines = []
        }

        func flushList() {
            guard !listItems.isEmpty else { return }
            append(.list(listItems))
            listItems = []
        }

        func flushAll() {
            flushParagraph()
            flushTable()
            flushQuote()
            flushList()
        }

        for line in text.split(separator: "\n", omittingEmptySubsequences: false) {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if inCode {
                if trimmed.hasPrefix("```") {
                    append(.code(code.joined(separator: "\n")))
                    code = []
                    inCode = false
                } else {
                    code.append(line)
                }
            } else if trimmed.hasPrefix("```") {
                flushAll()
                inCode = true
            } else if trimmed.hasPrefix("|") {
                flushParagraph()
                flushQuote()
                flushList()
                tableLines.append(line)
            } else if let (level, content) = heading(trimmed) {
                flushAll()
                append(.heading(level, content))
            } else if trimmed.hasPrefix(">") {
                flushParagraph()
                flushTable()
                flushList()
                quoteLines.append(quoteContent(trimmed))
            } else if let item = listItem(line) {
                flushParagraph()
                flushTable()
                flushQuote()
                listItems.append(item)
            } else {
                flushTable()
                flushQuote()
                flushList()
                paragraph.append(line)
            }
        }
        // An unterminated fence (still streaming) renders as code.
        if inCode {
            append(.code(code.joined(separator: "\n")))
        }
        flushAll()
        return result
    }

    private func quoteContent(_ line: String) -> String {
        let content = line.dropFirst()
        return content.hasPrefix(" ") ? String(content.dropFirst()) : String(content)
    }

    private func listItem(_ line: Substring) -> MessageBlock.ListItem? {
        let indent = line.prefix(while: { $0 == " " }).count / 2
        let trimmed = line.trimmingCharacters(in: .whitespaces)
        for bullet in ["- ", "* ", "+ "] where trimmed.hasPrefix(bullet) {
            return MessageBlock.ListItem(indent: indent, marker: "•", text: String(trimmed.dropFirst(2)))
        }
        let digits = trimmed.prefix(while: \.isNumber)
        guard !digits.isEmpty else { return nil }
        let rest = trimmed.dropFirst(digits.count)
        guard rest.hasPrefix(". ") || rest.hasPrefix(") ") else { return nil }
        return MessageBlock.ListItem(indent: indent, marker: "\(digits).", text: String(rest.dropFirst(2)))
    }

    private func heading(_ line: String) -> (Int, String)? {
        guard line.hasPrefix("#") else { return nil }
        let hashes = line.prefix(while: { $0 == "#" })
        guard hashes.count <= 6, line.dropFirst(hashes.count).first == " " else { return nil }
        return (hashes.count, String(line.dropFirst(hashes.count + 1)))
    }

    private func cells(_ line: Substring) -> [String] {
        var trimmed = line.trimmingCharacters(in: .whitespaces)
        if trimmed.hasPrefix("|") {
            trimmed.removeFirst()
        }
        if trimmed.hasSuffix("|") {
            trimmed.removeLast()
        }
        return trimmed.split(separator: "|", omittingEmptySubsequences: false)
            .map { $0.trimmingCharacters(in: .whitespaces) }
    }

    private func isSeparator(_ cells: [String]) -> Bool {
        !cells.isEmpty && cells.allSatisfy { cell in
            !cell.isEmpty && cell.allSatisfy { "-:".contains($0) }
        }
    }
}
