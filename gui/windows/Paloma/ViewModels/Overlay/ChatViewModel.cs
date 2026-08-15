using System.Collections.ObjectModel;
using System.Text;
using CommunityToolkit.Mvvm.ComponentModel;
using Paloma.Client;
using Paloma.Helpers;
using Paloma.Models;
using Serilog;
using PermissionState = Paloma.Binding.V1.PermissionState;
using ProviderBackendId = Paloma.Binding.V1.ProviderBackendId;
using UserDecision = Paloma.Binding.V1.UserDecision;

namespace Paloma.ViewModels.Overlay;

public sealed partial class ChatViewModel(IPalomaClient client, Func<Action, bool>? delayFlush = null)
    : ObservableObject, IDisposable
{
    private CancellationTokenSource? _turnCts;
    private string? _sessionId;
    private DecisionViewModel? _decisionCursor;

    public ObservableCollection<ChatSectionViewModel> Sections { get; } = [];

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(Streaming))]
    public partial ChatStatus Status { get; private set; } = ChatStatus.Idle;

    [ObservableProperty] public partial string StatusMessage { get; private set; } = string.Empty;

    public bool Streaming => Status == ChatStatus.Streaming;

    public bool CanSubmit(string prompt)
    {
        return !Streaming && prompt.Trim().Length > 0;
    }

    public async Task SubmitAsync(string prompt)
    {
        if (!CanSubmit(prompt))
        {
            return;
        }

        prompt = prompt.Trim();
        _turnCts?.Cancel();
        var turn = _turnCts = new CancellationTokenSource();
        Status = ChatStatus.Streaming;
        StatusMessage = string.Empty;
        ProviderBackendId? backend;
        try
        {
            backend = await client.PreferModelAsync(turn.Token);
        }
        catch (OperationCanceledException)
        {
            return;
        }
        catch (Exception e)
        {
            Fail(turn, PalomaClient.Describe(e));
            return;
        }

        if (backend is null)
        {
            Fail(turn, "No model selected. Connect a provider first.");
            return;
        }

        await ConsumeTurnAsync(client.ChatAsync(_sessionId, backend, prompt, turn.Token), turn);
    }

    public async Task RestoreAsync(string sessionId)
    {
        Clear();
        _sessionId = sessionId;
        var turn = _turnCts = new CancellationTokenSource();
        Status = ChatStatus.Streaming;
        await ConsumeTurnAsync(
            client.RestoreSessionAsync(sessionId, turn.Token),
            turn,
            onFailure: () =>
            {
                if (!turn.IsCancellationRequested)
                {
                    _sessionId = null;
                }
            });
    }

    public async Task InterruptAsync()
    {
        if (!Streaming)
        {
            return;
        }

        // Before SessionStarted arrives there is no session to cancel on
        // the server. Cancel the turn locally instead.
        if (_sessionId is null)
        {
            _turnCts?.Cancel();
            Status = ChatStatus.Cancelled;
            return;
        }

        try
        {
            await client.CancelSessionAsync(_sessionId);
        }
        catch (Exception e)
        {
            // Failure to cancel just lets the turn run out; nothing to surface.
            Log.Warning(e, "session cancel failed");
        }
    }

    public void Clear()
    {
        _turnCts?.Cancel();
        _turnCts = null;
        Sections.Clear();
        Status = ChatStatus.Idle;
        StatusMessage = string.Empty;
        _sessionId = null;
        _decisionCursor = null;
    }

    /// <summary>Moves the cursor across pending decisions and returns the
    /// owning section. Stepping above the first returns to the input.</summary>
    public ChatSectionViewModel? Navigate(int delta)
    {
        var pending = Sections
            .OfType<ToolSectionViewModel>()
            .Where(section => section.Unresolved)
            .SelectMany(section => section.Decisions)
            .ToList();
        if (pending.Count == 0)
        {
            return null;
        }

        var index = _decisionCursor is null ? -1 : pending.IndexOf(_decisionCursor);
        index = Math.Min(index + delta, pending.Count - 1);
        _decisionCursor?.IsSelected = false;

        if (index < 0)
        {
            _decisionCursor = null;
            return null;
        }

        _decisionCursor = pending[index];
        _decisionCursor.IsSelected = true;
        return Sections
            .OfType<ToolSectionViewModel>()
            .FirstOrDefault(section => section.Decisions.Contains(_decisionCursor));
    }

    public bool DecideSelected()
    {
        if (_decisionCursor is not { } cursor)
        {
            return false;
        }

        cursor.Decide();
        return true;
    }

    public void Dispose()
    {
        _turnCts?.Dispose();
    }

    private async Task ConsumeTurnAsync(
        IAsyncEnumerable<ChatStreamEvent> stream,
        CancellationTokenSource turn,
        Action? onFailure = null)
    {
        try
        {
            await foreach (var e in stream)
            {
                if (turn.IsCancellationRequested)
                {
                    return;
                }

                Render(e);
            }

            // in case the stream does not end with DONE/ERROR, explicitly mark it to idle
            if (!turn.IsCancellationRequested && Status == ChatStatus.Streaming)
            {
                Status = ChatStatus.Idle;
            }
        }
        catch (OperationCanceledException)
        {
        }
        catch (Exception e)
        {
            onFailure?.Invoke();
            Fail(turn, PalomaClient.Describe(e));
        }
    }

    private void Render(ChatStreamEvent e)
    {
        switch (e)
        {
            case ChatStreamEvent.SessionStarted started:
                _sessionId = started.SessionId;
                break;
            case ChatStreamEvent.UserPrompt prompt:
                Sections.Add(new UserSectionViewModel(prompt.Text));
                break;
            case ChatStreamEvent.TextDelta delta:
                if (Sections.LastOrDefault() is AssistantSectionViewModel assistant)
                {
                    assistant.Append(delta.Text);
                }
                else
                {
                    Sections.Add(
                        new AssistantSectionViewModel(delta.Backend, delta.Text, delayFlush));
                }

                break;
            case ChatStreamEvent.ReasoningDelta delta:
                if (Sections.LastOrDefault() is ReasoningSectionViewModel reasoning)
                {
                    reasoning.Append(delta.Text);
                }
                else
                {
                    Sections.Add(new ReasoningSectionViewModel(delta.Text, delayFlush));
                }

                break;
            case ChatStreamEvent.ToolCall call:
                Sections.Add(new ToolSectionViewModel(
                    client, call, ReportError, ResetCursor, OnDecisionResolved));
                break;
            case ChatStreamEvent.Done:
                Status = ChatStatus.Idle;
                break;
            case ChatStreamEvent.Cancelled:
                Status = ChatStatus.Cancelled;
                break;
            case ChatStreamEvent.Error error:
                Status = ChatStatus.Failed;
                ReportError(error.Message);
                break;
        }
    }

    private void ReportError(string message)
    {
        StatusMessage = message;
    }

    // Reset the current selection if the trigger one is the selection one
    private void ResetCursor(Func<DecisionViewModel, bool> contains)
    {
        if (_decisionCursor is not { } cursor || !contains(cursor)) return;
        cursor.IsSelected = false;
        _decisionCursor = null;
    }

    // Once ignore permission is clicked, autopopulate for all other decisions with same format.
    // This method will cause cascade, should be safe due to it is run in UI thread which is single threaded
    // and Deciding flag will gate on duplicate RPC calls.
    private void OnDecisionResolved(UserDecision decision)
    {
        if (decision.DecisionCase != UserDecision.DecisionOneofCase.IgnorePermission)
        {
            return;
        }

        foreach (var section in Sections.OfType<ToolSectionViewModel>())
        {
            if (!section.Unresolved)
            {
                continue;
            }

            section.Decisions
                .FirstOrDefault(other =>
                    other.Decision.DecisionCase == UserDecision.DecisionOneofCase.IgnorePermission)
                ?.Decide();
        }
    }

    private void Fail(CancellationTokenSource turn, string message)
    {
        if (turn.IsCancellationRequested)
        {
            return;
        }

        Status = ChatStatus.Failed;
        ReportError(message);
    }
}

public abstract class ChatSectionViewModel : ObservableObject;

public sealed partial class UserSectionViewModel(string text) : ChatSectionViewModel
{
    public string Text { get; } = text;
}

public abstract partial class StreamingSectionViewModel : ChatSectionViewModel
{
    private readonly StringBuilder _builder;

    // Holds a flush until the delta burst ends, so text renders once
    // instead of per delta; without a dispatcher the flush runs inline.
    private readonly BatchedAction _flush;

    [ObservableProperty] public partial string Text { get; private set; }

    private protected StreamingSectionViewModel(string text, Func<Action, bool>? delayFlush)
    {
        _builder = new StringBuilder(text);
        Text = text;
        _flush = new BatchedAction(() => Text = _builder.ToString(), delayFlush);
    }

    public void Append(string delta)
    {
        _builder.Append(delta);
        _flush.Trigger();
    }
}

public sealed partial class AssistantSectionViewModel(
    ProviderBackendId backend,
    string text,
    Func<Action, bool>? delayFlush = null) : StreamingSectionViewModel(text, delayFlush)
{
    public ProviderBackendId Backend { get; } = backend;
}

public sealed partial class ReasoningSectionViewModel(
    string text,
    Func<Action, bool>? delayFlush = null) : StreamingSectionViewModel(text, delayFlush)
{
    [ObservableProperty] public partial bool IsExpanded { get; set; }
}

public sealed partial class ToolSectionViewModel : ChatSectionViewModel
{
    private readonly Action<string> _reportError;
    private readonly Action<Func<DecisionViewModel, bool>> _cleanup;
    private readonly Action<UserDecision> _resolved;

    public string ToolName { get; }

    public string? ToolDescription { get; }

    public string Arguments { get; }

    public IReadOnlyList<DecisionViewModel> Decisions { get; }

    public IReadOnlyList<DecisionViewModel> AllowDecisions { get; }

    public IReadOnlyList<DecisionViewModel> TerminalDecisions { get; }

    public bool HasTerminalDecisions => TerminalDecisions.Count > 0;

    public bool HasDecisions => Decisions.Count > 0;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(Unresolved))]
    public partial PermissionState? Resolution { get; private set; }

    public bool Unresolved => HasDecisions && Resolution is null;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(CanDecide))]
    public partial bool Deciding { get; private set; }

    public bool CanDecide => !Deciding;

    public ToolSectionViewModel(
        IPalomaClient client,
        ChatStreamEvent.ToolCall call,
        Action<string> reportError,
        Action<Func<DecisionViewModel, bool>> cleanup,
        Action<UserDecision> resolved)
    {
        _reportError = reportError;
        _cleanup = cleanup;
        _resolved = resolved;
        ToolName = call.Name;
        ToolDescription = call.Description;
        Arguments = call.Arguments;
        Decisions = [.. call.Decisions.Select(decision => new DecisionViewModel(client, decision, OnDecide))];
        AllowDecisions = [.. Decisions.Where(decision => !decision.Terminal)];
        TerminalDecisions = [.. Decisions.Where(decision => decision.Terminal)];
    }

    private async void OnDecide(UserDecision decision, Func<Task<PermissionState>> decide)
    {
        if (Deciding || Resolution is not null)
        {
            return;
        }

        Deciding = true;
        try
        {
            Resolution = await decide();
        }
        catch (Exception e)
        {
            // Core never received the decision and still waits for one; keep
            // the buttons for a retry.
            _reportError(PalomaClient.Describe(e));
        }
        finally
        {
            Deciding = false;
            // A resolution retires the whole section, so a cursor parked on
            // a sibling clears too. A failed decision keeps its highlight.
            if (Resolution is not null)
            {
                _cleanup(Decisions.Contains);
                _resolved(decision);
            }
        }
    }
}

public sealed partial class DecisionViewModel(
    IPalomaClient client,
    UserDecision decision,
    Action<UserDecision, Func<Task<PermissionState>>> onDecide) : ObservableObject
{
    public UserDecision Decision { get; } = decision;

    public bool Terminal => Decision.DecisionCase
        is UserDecision.DecisionOneofCase.Deny
        or UserDecision.DecisionOneofCase.IgnorePermission;

    [ObservableProperty] public partial bool IsSelected { get; set; }

    public void Decide()
    {
        onDecide(Decision, DecideAsync);
        return;

        async Task<PermissionState> DecideAsync()
        {
            var result = await client.DecideAsync(Decision);
            IsSelected = false;
            return result;
        }
    }
}