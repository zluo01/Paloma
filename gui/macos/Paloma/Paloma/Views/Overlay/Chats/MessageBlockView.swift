//
//  MessageBlockView.swift
//  Paloma
//
//

import SwiftUI

struct MessageBlockView: View {
    let block: MessageBlock

    var body: some View {
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

    private func headingFont(_ level: Int) -> Font {
        switch level {
        case 1: .system(size: 17, weight: .bold)
        case 2: .system(size: 15, weight: .bold)
        case 3: .system(size: 14, weight: .semibold)
        default: .system(size: 13, weight: .semibold)
        }
    }

    private func inline(_ text: String) -> AttributedString {
        (try? AttributedString(
            markdown: text,
            options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace)
        )) ?? AttributedString(text)
    }
}
