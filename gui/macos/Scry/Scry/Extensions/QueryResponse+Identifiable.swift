//
//  QueryResponse+Identifiable.swift
//  Scry
//
//  Lives outside Generated/ so bindgen runs do not overwrite it.
//

extension QueryResponse: Identifiable {
    public var id: ExtensionCapabilityId {
        extensionCapabilityId
    }
}
