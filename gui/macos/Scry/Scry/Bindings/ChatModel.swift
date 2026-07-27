//
//  ChatModel.swift
//  Scry
//
//

import Foundation
import Observation

@MainActor
@Observable
final class ChatModel {
    private(set) var transcript: [ChatSection] = []
    private(set) var chatStatus: ChatStatus = .idle
    /// Bumped on every rendered chat event so the view can follow the tail.
    private(set) var chatRevision = 0
    /// Keyboard cursor over the pending decision buttons; -1 = input field.
    private(set) var decisionCursor = -1
    /// Tool ids with a decision round-trip in flight.
    private(set) var deciding: Set<Int> = []

    @ObservationIgnored private(set) var sessionId: Uuid?
    @ObservationIgnored private var chatTask: Task<Void, Never>?
    @ObservationIgnored private var sectionCounter = 0
    /// uniffi awaits ignore Swift cancellation; superseded continuations fence every write.
    @ObservationIgnored private var generation = 0

    private func isCurrent(_ turn: Int) -> Bool {
        turn == generation
    }

    func navigate(by delta: Int) {
        let count = pendingDecisions.count
        guard count > 0 else { return }
        decisionCursor = min(max(decisionCursor + delta, -1), count - 1)
    }

    func submitChat(_ query: String) {
        guard chatStatus != .streaming else {
            return
        }

        let prompt = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !prompt.isEmpty else { return }

        chatTask?.cancel()
        generation += 1
        let turn = generation
        chatStatus = .streaming

        chatTask = Task {
            let result = await CoreClient.shared.withApp { app in
                guard let providerBackendId = try await app.preferModel() else {
                    if isCurrent(turn) {
                        chatStatus = .failed("No model selected. Connect a provider first.")
                    }
                    return
                }
                guard isCurrent(turn) else { return }
                let chat = try await app.chat(
                    sessionId: sessionId,
                    providerBackendId: providerBackendId,
                    prompt: prompt
                )
                guard isCurrent(turn) else { return }
                sessionId = chat.sessionId()
                while let event = await chat.next() {
                    guard isCurrent(turn) else { return }
                    render(event)
                }
                if isCurrent(turn) {
                    finishStreamIfNeeded()
                }
            }
            if case let .failure(error) = result, isCurrent(turn) {
                chatStatus = .failed(error.displayMessage)
            }
        }
    }

    /// The stream delivers RenderEvent.cancel, which marks the transcript.
    func interrupt() {
        guard let sessionId, chatStatus == .streaming else { return }
        Task {
            // Failure to cancel just lets the turn run out; nothing to surface.
            _ = await CoreClient.shared.withApp { app in
                try await app.cancelSession(sessionId: sessionId)
            }
        }
    }

    /// Drops view state only; a running turn finishes server-side into the stored session.
    func clear() {
        generation += 1
        chatTask?.cancel()
        transcript = []
        chatStatus = .idle
        sessionId = nil
        sectionCounter = 0
        decisionCursor = -1
        deciding = []
    }

    func restore(sessionId: Uuid) {
        clear()
        self.sessionId = sessionId
        chatStatus = .streaming
        let turn = generation

        chatTask = Task {
            let result = await CoreClient.shared.withApp { app in
                let stream = try await app.restoreSession(sessionId: sessionId)
                while let event = await stream.next() {
                    guard isCurrent(turn) else { return }
                    render(event)
                }
                if isCurrent(turn) {
                    finishStreamIfNeeded()
                }
            }
            if case let .failure(error) = result, isCurrent(turn) {
                // The failed session must not stay current, or the next
                // prompt would target it.
                self.sessionId = nil
                chatStatus = .failed(error.displayMessage)
            }
        }
    }

    func decide(_ decision: UserDecision, toolId: Int) {
        guard deciding.insert(toolId).inserted else { return }
        let turn = generation
        Task {
            let result = await CoreClient.shared.withApp { app in
                try await app.decideToolcallPermissions(userDecision: decision)
            }
            deciding.remove(toolId)
            guard isCurrent(turn) else { return }
            resolveTool(toolId, with: (try? result.get()) ?? .error)
        }
    }

    private func resolveTool(_ toolId: Int, with state: PermissionState) {
        guard let index = transcript.firstIndex(where: { $0.id == toolId }),
              case var .tool(tool) = transcript[index]
        else {
            return
        }
        tool.resolution = state
        transcript[index] = .tool(tool)
        decisionCursor = -1
        chatRevision += 1
    }

    // MARK: Decision keyboard navigation

    struct PendingDecision: Equatable {
        let toolId: Int
        let index: Int
        let decision: UserDecision
    }

    /// All decision buttons of unresolved tool calls, in transcript order.
    var pendingDecisions: [PendingDecision] {
        transcript.flatMap { section -> [PendingDecision] in
            guard case let .tool(tool) = section, tool.resolution == nil else {
                return []
            }
            return tool.decisions.enumerated().map { index, decision in
                PendingDecision(toolId: tool.id, index: index, decision: decision)
            }
        }
    }

    var selectedDecision: PendingDecision? {
        let pending = pendingDecisions
        guard decisionCursor >= 0, decisionCursor < pending.count else { return nil }
        return pending[decisionCursor]
    }

    func isDecisionSelected(toolId: Int, index: Int) -> Bool {
        guard let selected = selectedDecision else { return false }
        return selected.toolId == toolId && selected.index == index
    }

    /// Enter with a decision selected activates it instead of sending text.
    func decideSelected() -> Bool {
        guard let selected = selectedDecision else { return false }
        decide(selected.decision, toolId: selected.toolId)
        return true
    }

    private func render(_ event: RenderEvent) {
        switch event {
        case let .chat(event):
            renderChat(event)
        case .done:
            chatStatus = .idle
        case .cancel:
            chatStatus = .cancelled
        case let .error(message):
            chatStatus = .failed(message)
        case .search:
            break
        }
        chatRevision += 1
    }

    /// Deltas accumulate into the trailing section of the same kind.
    private func renderChat(_ event: ChatRenderEvent) {
        switch event {
        case let .userPrompt(text):
            transcript.append(.user(id: nextSectionId(), text: text))
        case let .textDelta(providerBackendId, text):
            if case let .assistant(id, current, existing) = transcript.last,
               current == providerBackendId
            {
                transcript[transcript.count - 1] =
                    .assistant(id: id, providerBackendId: providerBackendId, text: existing + text)
            } else {
                transcript.append(.assistant(id: nextSectionId(), providerBackendId: providerBackendId, text: text))
            }
        case let .reasoningDelta(text):
            if case let .reasoning(id, existing) = transcript.last {
                transcript[transcript.count - 1] = .reasoning(id: id, text: existing + text)
            } else {
                transcript.append(.reasoning(id: nextSectionId(), text: text))
            }
        case let .toolCall(toolName, arguments, description, decisions):
            transcript.append(
                .tool(
                    ToolCallState(
                        id: nextSectionId(),
                        name: toolName,
                        arguments: arguments,
                        description: description,
                        decisions: decisions,
                        resolution: nil
                    )
                )
            )
        }
    }

    private func finishStreamIfNeeded() {
        if chatStatus == .streaming {
            chatStatus = .idle
        }
    }

    private func nextSectionId() -> Int {
        sectionCounter += 1
        return sectionCounter
    }
}
