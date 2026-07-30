//
//  SearchView.swift
//  Paloma
//

import SwiftUI

/// Selected-row bounds let the action panel render outside the ScrollView clip.
struct SelectedRowBounds: PreferenceKey {
    static let defaultValue: Anchor<CGRect>? = nil
    static func reduce(value: inout Anchor<CGRect>?, nextValue: () -> Anchor<CGRect>?) {
        value = nextValue() ?? value
    }
}

struct SearchView: View {
    let query: String
    let sections: [QueryResponse]
    let bases: [Int]
    let selection: Int
    let panelSelection: Int?
    let selectedItem: Item?
    let chatRowSelected: Bool
    let onEvent: (SearchEvent) -> Void

    var body: some View {
        if !sections.isEmpty {
            results
        }
    }

    private var results: some View {
        ScrollViewReader { proxy in
            CappedScrollView {
                VStack(alignment: .leading, spacing: 2) {
                    ForEach(Array(sections.enumerated()), id: \.element.id) { sectionIndex, section in
                        let base = bases[sectionIndex]
                        Text(section.name)
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(.secondary)
                            .padding(.horizontal, 14)
                            .padding(.top, 8)
                        ForEach(Array(section.items.enumerated()), id: \.offset) { itemIndex, item in
                            let index = base + itemIndex
                            SearchRowView(
                                item: item,
                                index: index,
                                selected: index == selection,
                                actionHint: index == selection && panelSelection == nil
                                    && item.actions.count > 1,
                                onEvent: onEvent
                            )
                            .id(index)
                        }
                    }
                    chatRow
                }
                .padding(.horizontal, 8)
                .padding(.bottom, 8)
                .id("results")
            }
            .onChange(of: selection) {
                // The extremes snap the whole card into view.
                if selection == 0 {
                    proxy.scrollTo("results", anchor: .top)
                } else if chatRowSelected {
                    proxy.scrollTo("results", anchor: .bottom)
                } else {
                    proxy.scrollTo(selection)
                }
            }
            .overlayPreferenceValue(SelectedRowBounds.self) { anchor in
                GeometryReader { geometry in
                    if let anchor, let panelSelection, let selectedItem {
                        let row = geometry[anchor]
                        let estimated = CGFloat(selectedItem.actions.count) * 25 + 8
                        let fitsBelow = row.maxY + estimated <= geometry.size.height
                        let fitsAbove = row.minY - estimated >= 0
                        ActionPanelView(
                            actions: selectedItem.actions,
                            selection: panelSelection,
                            onEvent: onEvent
                        )
                        .frame(width: row.width)
                        .offset(
                            x: row.minX,
                            y: fitsBelow || !fitsAbove ? row.maxY : row.minY - estimated
                        )
                    }
                }
            }
        }
    }

    private var chatRow: some View {
        HStack(spacing: 10) {
            IconView(systemName: "sparkles")
            Text("Chat about \u{201C}\(query)\u{201D}")
                .font(.system(size: 14))
                .lineLimit(1)
            Spacer(minLength: 0)
        }
        .searchRow(selected: chatRowSelected) {
            onEvent(.chat)
        }
        .padding(.top, 6)
    }
}

private struct ActionPanelView: View {
    let actions: [Action]
    let selection: Int
    let onEvent: (SearchEvent) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            ForEach(Array(actions.enumerated()), id: \.offset) { actionIndex, action in
                Text(action.label)
                    .font(.system(size: 13))
                    .lineLimit(1)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 8)
                    .padding(.vertical, 4)
                    .rowHighlight(actionIndex == selection, cornerRadius: 6)
                    .contentShape(Rectangle())
                    .onTapGesture {
                        onEvent(.subAction(index: actionIndex))
                    }
            }
        }
        .padding(4)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 8))
        .shadow(color: .black.opacity(0.25), radius: 8, y: 2)
    }
}
