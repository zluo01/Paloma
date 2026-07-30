//
//  ChatStatus.swift
//  Paloma
//
//

enum ChatStatus: Equatable {
    case idle
    case streaming
    case cancelled
    case failed(String)
}
