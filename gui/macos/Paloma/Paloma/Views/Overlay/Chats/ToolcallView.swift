//
//  ToolcallView.swift
//  Paloma
//
//

import SwiftUI

struct ToolCallView: View {
    let tool: ToolCallState
    let model: ChatModel

    @State private var expanded = false

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
                    .lineLimit(expanded ? nil : 2)
                    .contentShape(.rect)
                    .onTapGesture { expanded.toggle() }
                    .help(expanded ? "Collapse" : "Expand")
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
