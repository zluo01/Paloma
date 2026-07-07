//
//  ScryError+Message.swift
//  Scry
//
//  Interpolating the error directly renders the case wrapper ("Failure(message: ...)").
//

extension ScryError {
    var message: String {
        switch self {
        case let .Failure(message):
            message
        }
    }
}

extension Error {
    var displayMessage: String {
        (self as? ScryError)?.message ?? "\(self)"
    }
}
