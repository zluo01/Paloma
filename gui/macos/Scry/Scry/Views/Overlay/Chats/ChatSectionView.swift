//
//  ChatSectionView.swift
//  Scry
//
//

import SwiftUI

struct ChatSectionView: View {
    let section: ChatSection
    let model: ChatModel

    var body: some View {
        switch section {
        case let .user(_, text):
            HStack {
                Spacer(minLength: 60)
                Text(text)
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
}

/// Inline-only AttributedString cannot express blocks, so lines split into blocks first.
private struct MarkdownBlock: Identifiable {
    enum Kind {
        case paragraph(String)
        case heading(Int, String)
        case code(String)
        case table(header: [String], rows: [[String]])
        case quote(String)
        case list([ListItem])
    }

    let id: Int
    let kind: Kind
}

private struct ListItem {
    let indent: Int
    let marker: String
    let text: String
}

private struct MessageText: View {
    let text: String

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            ForEach(blocks) { block in
                blockView(block)
            }
        }
    }

    @ViewBuilder
    private func blockView(_ block: MarkdownBlock) -> some View {
        switch block.kind {
        case let .paragraph(content):
            Text(inline(content))
                .font(.system(size: 13))
                .textSelection(.enabled)
        case let .heading(level, content):
            Text(inline(content))
                .font(headingFont(level))
                .textSelection(.enabled)
                .padding(.top, 4)
        case let .code(content):
            Text(content)
                .font(.system(size: 12, design: .monospaced))
                .textSelection(.enabled)
                .padding(8)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(.quaternary.opacity(0.5), in: RoundedRectangle(cornerRadius: 8))
        case let .quote(content):
            HStack(alignment: .top, spacing: 8) {
                RoundedRectangle(cornerRadius: 1)
                    .fill(.quaternary)
                    .frame(width: 3)
                Text(inline(content))
                    .font(.system(size: 13))
                    .foregroundStyle(.secondary)
                    .textSelection(.enabled)
            }
            .fixedSize(horizontal: false, vertical: true)
        case let .list(items):
            VStack(alignment: .leading, spacing: 3) {
                ForEach(items.indices, id: \.self) { index in
                    HStack(alignment: .firstTextBaseline, spacing: 6) {
                        Text(items[index].marker)
                            .font(.system(size: 13))
                            .foregroundStyle(.secondary)
                        Text(inline(items[index].text))
                            .font(.system(size: 13))
                            .textSelection(.enabled)
                    }
                    .padding(.leading, CGFloat(items[index].indent) * 14)
                }
            }
        case let .table(header, rows):
            Grid(alignment: .leading, horizontalSpacing: 12, verticalSpacing: 4) {
                if !header.isEmpty {
                    GridRow {
                        ForEach(header.indices, id: \.self) { column in
                            Text(inline(header[column]))
                                .font(.system(size: 12, weight: .semibold))
                        }
                    }
                    Divider()
                }
                ForEach(rows.indices, id: \.self) { row in
                    GridRow {
                        ForEach(rows[row].indices, id: \.self) { column in
                            Text(inline(rows[row][column]))
                                .font(.system(size: 12))
                        }
                    }
                }
            }
            .textSelection(.enabled)
            .padding(8)
            .background(.quaternary.opacity(0.5), in: RoundedRectangle(cornerRadius: 8))
        }
    }

    private var blocks: [MarkdownBlock] {
        var result: [MarkdownBlock] = []
        var paragraph: [Substring] = []
        var code: [Substring] = []
        var tableLines: [Substring] = []
        var quoteLines: [String] = []
        var listItems: [ListItem] = []
        var inCode = false

        func append(_ kind: MarkdownBlock.Kind) {
            result.append(MarkdownBlock(id: result.count, kind: kind))
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

    private func listItem(_ line: Substring) -> ListItem? {
        let indent = line.prefix(while: { $0 == " " }).count / 2
        let trimmed = line.trimmingCharacters(in: .whitespaces)
        for bullet in ["- ", "* ", "+ "] where trimmed.hasPrefix(bullet) {
            return ListItem(indent: indent, marker: "•", text: String(trimmed.dropFirst(2)))
        }
        let digits = trimmed.prefix(while: \.isNumber)
        guard !digits.isEmpty else { return nil }
        let rest = trimmed.dropFirst(digits.count)
        guard rest.hasPrefix(". ") || rest.hasPrefix(") ") else { return nil }
        return ListItem(indent: indent, marker: "\(digits).", text: String(rest.dropFirst(2)))
    }

    private func heading(_ line: String) -> (Int, String)? {
        guard line.hasPrefix("#") else { return nil }
        let hashes = line.prefix(while: { $0 == "#" })
        guard hashes.count <= 6, line.dropFirst(hashes.count).first == " " else { return nil }
        return (hashes.count, String(line.dropFirst(hashes.count + 1)))
    }

    private func headingFont(_ level: Int) -> Font {
        switch level {
        case 1: .system(size: 17, weight: .bold)
        case 2: .system(size: 15, weight: .bold)
        case 3: .system(size: 14, weight: .semibold)
        default: .system(size: 13, weight: .semibold)
        }
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

    private func inline(_ text: String) -> AttributedString {
        (try? AttributedString(
            markdown: text,
            options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace)
        )) ?? AttributedString(text)
    }
}

private struct ReasoningView: View {
    let text: String
    @State private var expanded = false

    var body: some View {
        DisclosureGroup(isExpanded: $expanded) {
            Text(text)
                .font(.system(size: 12))
                .foregroundStyle(.secondary)
                .textSelection(.enabled)
        } label: {
            Text("Thinking")
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)
        }
    }
}

private struct ToolCallView: View {
    let tool: ToolCallState
    let model: ChatModel

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 6) {
                Image(systemName: "wrench.and.screwdriver")
                    .font(.caption)
                Text(tool.name)
                    .font(.system(size: 12, weight: .semibold, design: .monospaced))
                Spacer()
                resolutionBadge
            }
            if let description = tool.description, !description.isEmpty {
                Text(description)
                    .font(.system(size: 12))
                    .foregroundStyle(.secondary)
            }
            if !tool.arguments.isEmpty {
                Text(tool.arguments)
                    .font(.system(size: 11, design: .monospaced))
                    .foregroundStyle(.secondary)
                    .lineLimit(6)
                    .textSelection(.enabled)
            }
            if tool.resolution == nil, !tool.decisions.isEmpty {
                decisionButtons
            }
        }
        .padding(10)
        .background(.quaternary.opacity(0.5), in: RoundedRectangle(cornerRadius: 10))
    }

    @ViewBuilder
    private var resolutionBadge: some View {
        switch tool.resolution {
        case .allow:
            Text("Allowed").font(.caption).foregroundStyle(.green)
        case .deny:
            Text("Denied").font(.caption).foregroundStyle(.red)
        case .error:
            Text("Error").font(.caption).foregroundStyle(.orange)
        case nil:
            EmptyView()
        }
    }

    private var decisionButtons: some View {
        // Original indices are kept so keyboard selection lines up.
        let (allows, terminals) = split(tool.decisions)
        return VStack(alignment: .leading, spacing: 4) {
            ForEach(allows, id: \.0) { index, decision in
                decisionButton(decision, index: index, role: nil)
            }
            if !terminals.isEmpty {
                Divider()
                ForEach(terminals, id: \.0) { index, decision in
                    decisionButton(decision, index: index, role: .destructive)
                }
            }
        }
    }

    private func decisionButton(_ decision: UserDecision, index: Int, role: ButtonRole?)
        -> some View
    {
        Button(role: role) {
            model.decide(decision, toolId: tool.id)
        } label: {
            Text(label(for: decision))
                .font(.system(size: 12))
        }
        .buttonStyle(.bordered)
        .controlSize(.small)
        .disabled(model.deciding.contains(tool.id))
        .overlay(
            RoundedRectangle(cornerRadius: 6)
                .strokeBorder(
                    model.isDecisionSelected(toolId: tool.id, index: index)
                        ? AnyShapeStyle(.tint) : AnyShapeStyle(.clear),
                    lineWidth: 2
                )
        )
    }

    private func split(_ decisions: [UserDecision]) -> (
        [(Int, UserDecision)], [(Int, UserDecision)]
    ) {
        var allows: [(Int, UserDecision)] = []
        var terminals: [(Int, UserDecision)] = []
        for (index, decision) in decisions.enumerated() {
            switch decision {
            case .allowOnce, .allow, .allowSession:
                allows.append((index, decision))
            case .deny, .ignorePermission:
                terminals.append((index, decision))
            }
        }
        return (allows, terminals)
    }

    private func label(for decision: UserDecision) -> String {
        switch decision {
        case .allowOnce:
            "Allow once"
        case let .allow(_, command, glob):
            glob ? "Always allow \(command) *" : "Always allow \(command)"
        case .allowSession:
            "Allow for this session"
        case .ignorePermission:
            "Stop asking this session"
        case .deny:
            "Deny"
        }
    }
}
