//
//  View+OperationErrorAlert.swift
//  Paloma
//

import SwiftUI

extension View {
    func operationErrorAlert(_ error: Binding<OperationError?>) -> some View {
        alert(
            error.wrappedValue?.title ?? "",
            isPresented: Binding(
                get: { error.wrappedValue != nil },
                set: { shown in
                    if !shown {
                        error.wrappedValue = nil
                    }
                }
            )
        ) {
            Button("Close", role: .cancel) {}
        } message: {
            Text(error.wrappedValue?.message ?? "")
        }
    }
}
