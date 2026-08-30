using ProviderBackendId = PalomaCore.ProviderBackendId;
using UserDecision = PalomaCore.UserDecision;

namespace Paloma.Models;

public enum ChatStatus
{
    Idle,
    Streaming,
    Cancelled,
    Failed,
}

// Unified model for both chat and restore
public abstract record ChatStreamEvent
{
    public sealed record SessionStarted(string SessionId) : ChatStreamEvent;

    public sealed record UserPrompt(string Text) : ChatStreamEvent;

    public sealed record TextDelta(ProviderBackendId Backend, string Text) : ChatStreamEvent;

    public sealed record ReasoningDelta(string Text) : ChatStreamEvent;

    public sealed record ToolCall(
        string Name,
        string Arguments,
        string? Description,
        IReadOnlyList<UserDecision> Decisions) : ChatStreamEvent;

    public sealed record Done : ChatStreamEvent;

    public sealed record Cancelled : ChatStreamEvent;

    public sealed record Error(string Message) : ChatStreamEvent;
}