//
//  PermissionsView.swift
//  Scry
//

import SwiftUI

struct PermissionsView: View {
    let model: PermissionModel
    @State private var search = ""
    @State private var operationError: OperationError?

    var body: some View {
        Form {
            if visible.isEmpty {
                Text(
                    model.permissions.isEmpty ? "No saved permissions." : "No permissions match the search."
                )
                .foregroundStyle(.secondary)
            }
            ForEach(groups, id: \.0) { section, permissions in
                Section(section) {
                    ForEach(permissions, id: \.prefix) { permission in
                        HStack {
                            VStack(alignment: .leading, spacing: 2) {
                                Text(permission.prefix)
                                    .lineLimit(2)
                                Text(permission.withGlob ? "Glob match" : "Exact command")
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                            Spacer()
                            Button {
                                OperationError.run("Failed to Delete Permission", into: $operationError) {
                                    await model.delete(permission.prefix)
                                }
                            } label: {
                                Image(systemName: "trash")
                            }
                            .buttonStyle(.ghostIcon)
                            .help("Delete")
                        }
                    }
                }
            }
        }
        .formStyle(.grouped)
        .searchable(text: $search, placement: .toolbar, prompt: "Filter permissions")
        .operationErrorAlert($operationError)
    }

    private var visible: [Permission] {
        let needle = search.trimmingCharacters(in: .whitespaces).lowercased()
        guard !needle.isEmpty else { return model.permissions }
        return model.permissions.filter { permission in
            permission.prefix.lowercased().contains(needle)
                || (permission.withGlob ? "glob" : "exact").contains(needle)
        }
    }

    private var groups: [(String, [Permission])] {
        let grouped = Dictionary(grouping: visible) { permission in
            permission.prefix.split(separator: " ").first.map(String.init) ?? permission.prefix
        }
        return grouped.sorted { $0.key < $1.key }
    }
}
