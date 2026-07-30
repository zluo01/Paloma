//
//  ExtensionCapabilitiesView.swift
//  Scry
//

import SwiftUI

struct ExtensionCapabilitiesView: View {
    let extensionInfo: ExtensionInfo
    let onBack: () -> Void
    let onToggleCapability: (_ capability: String, _ facet: CapabilityFacet, _ disabled: Bool) -> Void

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
                    ForEach(searchCapabilities, id: \.id) { capabilityRow($0, facet: .search) }
                } header: {
                    Text("Search")
                }
            }
            if !toolCapabilities.isEmpty {
                Section {
                    ForEach(toolCapabilities, id: \.id) { capabilityRow($0, facet: .tool) }
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

    private var searchCapabilities: [CapabilityInfo] {
        capabilities(with: .search)
    }

    private var toolCapabilities: [CapabilityInfo] {
        capabilities(with: .tool)
    }

    private func capabilities(with facet: CapabilityFacet) -> [CapabilityInfo] {
        extensionInfo.capabilities.filter { $0.facets.contains { $0.facet == facet } }
    }

    private func capabilityRow(_ capability: CapabilityInfo, facet: CapabilityFacet) -> some View {
        CapabilityRowView(
            capability: capability,
            facet: facet,
            // Built-ins have no config to disable, so their switches stay live.
            isPluginDisabled: extensionInfo.config?.disabled ?? false
        ) { disabled in
            onToggleCapability(capability.id, facet, disabled)
        }
    }
}
