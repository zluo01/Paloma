//
//  PalomaError+Message.swift
//  Paloma
//
//  Interpolating the error directly renders the case wrapper ("Failure(message: ...)").
//

extension PalomaError {
    var message: String {
        switch self {
        case let .Failure(message):
            message
        }
    }
}

extension Error {
    var displayMessage: String {
        (self as? PalomaError)?.message ?? "\(self)"
    }
}
