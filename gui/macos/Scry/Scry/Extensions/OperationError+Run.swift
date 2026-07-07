//
//  OperationError+Run.swift
//  Scry
//

import SwiftUI

extension OperationError {
    @MainActor
    static func run<T>(
        _ title: String,
        into destination: Binding<OperationError?>,
        _ operation: @escaping () async -> Result<T, Error>,
        onSuccess: ((T) -> Void)? = nil,
        onFailure: (() -> Void)? = nil
    ) {
        Task {
            switch await operation() {
            case let .success(value):
                onSuccess?(value)
            case let .failure(error):
                destination.wrappedValue = OperationError(
                    title: title,
                    message: error.displayMessage
                )
                onFailure?()
            }
        }
    }

    @MainActor
    static func run(
        _ title: String,
        into destination: Binding<OperationError?>,
        _ operation: @escaping () async -> Result<Void, Error>,
        onSuccess: (() -> Void)? = nil,
        onFailure: (() -> Void)? = nil
    ) {
        run(
            title, into: destination, operation,
            onSuccess: { (_: Void) in onSuccess?() },
            onFailure: onFailure
        )
    }
}
