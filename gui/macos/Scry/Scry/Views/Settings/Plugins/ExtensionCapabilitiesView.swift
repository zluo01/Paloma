//
//  ExtensionCapabilitiesView.swift
//  Scry
//

import SwiftUI

struct ExtensionCapabilitiesView: View {
    let extensionInfo: ExtensionInfo
    let onBack: () -> Void

    var body: some View {
        Form {
            Section {
                if !extensionInfo.description.isEmpty {
                    Text(extensionInfo.description)
                        .foregroundStyle(.secondary)
                }
                if let author = extensionInfo.author {
                    LabeledContent("Author", value: author)
                }
                if let homepage = extensionInfo.homepage {
                    LabeledContent("Homepage") {
                        if let url = URL(string: homepage) {
                            Link(homepage, destination: url)
                        } else {
                            Text(homepage)
                        }
                    }
                }
                if let error = extensionInfo.error {
                    LabeledContent("Error", value: error)
                        .foregroundStyle(.red)
                }
            }
            if !searchCapabilities.isEmpty {
                Section {
                    ForEach(searchCapabilities, id: \.capabilityId, content: capabilityRow)
                } header: {
                    Text("Search")
                }
            }
            if !toolCapabilities.isEmpty {
                Section {
                    ForEach(toolCapabilities, id: \.capabilityId, content: capabilityRow)
                } header: {
                    Text("Tools")
                }
            }
        }
        .formStyle(.grouped)
        .navigationTitle(extensionInfo.name)
        .toolbar {
            ToolbarItem(placement: .navigation) {
                Button(action: onBack) {
                    Image(systemName: "chevron.left")
                }
                .help("Back to Plugins")
            }
        }
    }

    private var searchCapabilities: [Capability] {
        extensionInfo.capabilities.filter(\.search)
    }

    private var toolCapabilities: [Capability] {
        extensionInfo.capabilities.filter(\.tool)
    }

    private func capabilityRow(_ capability: Capability) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(capability.capabilityId)
            if !capability.description.isEmpty {
                Text(capability.description)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.vertical, 2)
    }
}
