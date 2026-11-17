//
//  RemoteAuth+Kind.swift
//  Paloma
//
//

extension RemoteAuth {
    /// One of each method with empty input, in the order the picker lists them.
    static let kinds: [RemoteAuth] = [
        .none,
        .bearer(token: ""),
        .headers(headers: [:]),
        .oauth,
        .oauthClient(clientId: ""),
    ]

    var label: String {
        switch self {
        case .none: "None"
        case .bearer: "Bearer token"
        case .headers: "Custom headers"
        case .oauth: "OAuth"
        case .oauthClient: "OAuth with client ID"
        }
    }

    var kind: RemoteAuth {
        get {
            switch self {
            case .none, .oauth: self
            case .bearer: .bearer(token: "")
            case .headers: .headers(headers: [:])
            case .oauthClient: .oauthClient(clientId: "")
            }
        }
        set {
            if newValue != kind {
                self = newValue
            }
        }
    }
}
