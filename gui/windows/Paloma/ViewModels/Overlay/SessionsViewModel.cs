using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using Paloma.Client;
using Paloma.Helpers;
using SessionListItem = PalomaCore.SessionListItem;

namespace Paloma.ViewModels.Overlay;

public sealed partial class SessionsViewModel(IPalomaClient client) : ObservableObject, IDisposable
{
    private CancellationTokenSource? _searchCts;
    private IReadOnlyList<SessionListItem> _sessions = [];
    private HashSet<string>? _filterIds;
    private int _selection = -1;

    public ObservableCollection<SessionRow> Rows { get; } = [];

    [ObservableProperty] public partial string Status { get; private set; } = string.Empty;

    public SessionRow? Selected =>
        _selection >= 0 && _selection < Rows.Count ? Rows[_selection] : null;

    public async Task LoadAsync()
    {
        await ClientGuard.TryAsync(
            async () =>
            {
                _sessions = await client.GetSessionsAsync();
                _filterIds = null;
                Apply(_sessions);
            },
            message => Status = message,
            "Failed to load sessions");
    }

    public async Task SearchAsync(string needle)
    {
        needle = needle.Trim();
        if (needle.Length == 0)
        {
            // clear any pending search
            _searchCts?.Cancel();
            _searchCts = null;
            _filterIds = null;
            Apply(_sessions);
            return;
        }

        // cancel any previous search
        _searchCts?.Cancel();
        var cts = _searchCts = new CancellationTokenSource();

        try
        {
            var ids = await client.SearchSessionsAsync(needle, cts.Token);
            if (cts.IsCancellationRequested)
            {
                return;
            }

            _filterIds = [.. ids];
            Apply(Visible());
        }
        catch (Exception e) when (PalomaClient.IsCancellation(e))
        {
        }
        catch (Exception e)
        {
            Status = $"Failed to search sessions: {PalomaClient.Describe(e)}";
        }
    }

    public int Move(int delta)
    {
        Select(_selection + delta);
        return _selection;
    }

    public void PendingDelete()
    {
        if (Selected is { } row)
        {
            PendingDelete(row);
        }
    }

    public void PendingDelete(SessionRow row)
    {
        // Select first so Enter and Escape operate on the clicked row.
        Select(Rows.IndexOf(row));
        row.PendingDeletion = true;
    }

    public bool CancelPendingDelete()
    {
        if (Selected is not { PendingDeletion: true } row) return false;
        row.PendingDeletion = false;
        return true;
    }

    public async Task<bool> ConfirmPendingDeleteAsync()
    {
        if (Selected is not { PendingDeletion: true } row)
        {
            return false;
        }

        var index = _selection;
        if (await RemoveAsync(row))
        {
            // Keep the keyboard flow anchored on the row above the deleted one.
            Select(index - 1);
        }

        return true;
    }

    public async Task<bool> RemoveAsync(SessionRow row)
    {
        return await ClientGuard.TryAsync(
            async () =>
            {
                await client.RemoveSessionAsync(row.Item.SessionId);
                _sessions = [.. _sessions.Where(session => session.SessionId != row.Item.SessionId)];
                Apply(Visible());
            },
            message => Status = message,
            "Failed to remove session");
    }

    public void Dispose()
    {
        _searchCts?.Dispose();
    }

    private void Apply(IReadOnlyList<SessionListItem> sessions)
    {
        Rows.Clear();
        _selection = -1;
        foreach (var session in sessions)
        {
            Rows.Add(new SessionRow(session));
        }

        if (Rows.Count > 0)
        {
            Move(1);
        }

        Status = (_sessions.Count, Rows.Count) switch
        {
            (0, _) => "No stored sessions.",
            (_, 0) => "No sessions match the search.",
            _ => string.Empty,
        };
    }

    private void Select(int index)
    {
        if (Rows.Count == 0)
        {
            return;
        }

        index = Math.Clamp(index, 0, Rows.Count - 1);
        if (Selected is { } previous)
        {
            previous.IsSelected = false;
            previous.PendingDeletion = false;
        }

        _selection = index;
        Rows[index].IsSelected = true;
    }

    private IReadOnlyList<SessionListItem> Visible()
    {
        return _filterIds is null
            ? _sessions
            : [.. _sessions.Where(session => _filterIds.Contains(session.SessionId))];
    }
}

public sealed partial class SessionRow(SessionListItem item) : ObservableObject
{
    public SessionListItem Item { get; } = item;

    [ObservableProperty] public partial bool IsSelected { get; set; }

    [ObservableProperty] public partial bool PendingDeletion { get; set; }

    [ObservableProperty] public partial bool IsHovered { get; set; }
}