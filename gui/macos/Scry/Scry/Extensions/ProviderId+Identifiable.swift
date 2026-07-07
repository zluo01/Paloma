//
//  ProviderId+Identifiable.swift
//  Scry
//
//  Lives outside Generated/ so bindgen runs do not overwrite it.
//

extension ProviderId: Identifiable {
    public var id: Self {
        self
    }
}
