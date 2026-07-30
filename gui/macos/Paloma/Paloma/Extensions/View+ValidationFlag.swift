//
//  View+ValidationFlag.swift
//  Paloma
//

import SwiftUI

extension View {
    func validationFlag(_ error: String?) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            overlay(
                RoundedRectangle(cornerRadius: 5)
                    .strokeBorder(.red.opacity(0.8), lineWidth: 3)
                    .opacity(error == nil ? 0 : 1)
            )
            if let error {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(.red)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }
}
