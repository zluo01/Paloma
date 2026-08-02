using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Messaging;
using Microsoft.UI.Xaml.Controls;
using Paloma.Client;
using Paloma.Helpers;
using Paloma.Messages;
using CapabilityIcon = Paloma.Extension.V1.CapabilityIcon;
using ExtAction = Paloma.Extension.V1.Action;
using ExtensionCapabilityId = Paloma.Binding.V1.ExtensionCapabilityId;
using Item = Paloma.Extension.V1.Item;
using RunActionResponse = Paloma.Extension.V1.RunActionResponse;

namespace Paloma.ViewModels.Overlay;

public sealed partial class SearchViewModel(IPalomaClient client, IMessenger? messenger = null)
    : ObservableObject, IDisposable
{
    private readonly IMessenger _messenger = messenger ?? WeakReferenceMessenger.Default;
    private CancellationTokenSource? _searchCts;
    private bool _activating;
    private int _selection = -1;

    public ObservableCollection<SearchGroup> Groups { get; } = [];

    [ObservableProperty] public partial bool HasResults { get; private set; }

    [ObservableProperty] public partial bool AskSelected { get; private set; }

    [ObservableProperty] public partial string Query { get; private set; } = string.Empty;

    public LauncherRow? SelectedRow => RowAt(_selection);

    public int RowCount => Groups.Sum(group => group.Items.Count);

    public async Task SearchAsync(string query)
    {
        query = query.Trim();
        if (query.Length == 0)
        {
            // clear any pending search
            _searchCts?.Cancel();
            _searchCts = null;
            Reset();
            return;
        }

        // cancel any previous search
        _searchCts?.Cancel();
        var cts = _searchCts = new CancellationTokenSource();
        // delay the first data appearances gap,
        // we will clear the old stale data until we get first section to render
        var stale = true;
        try
        {
            await foreach (var section in client.SearchAsync(query, cts.Token))
            {
                if (cts.IsCancellationRequested)
                {
                    break;
                }

                // Rows without actions cannot be activated.
                var items = section.Items.Where(item => item.Actions.Count > 0).ToList();
                if (items.Count == 0)
                {
                    continue;
                }

                if (stale)
                {
                    Reset();
                    Query = query;
                    HasResults = true;
                    stale = false;
                }

                Groups.Add(new SearchGroup(
                    section.Name,
                    [.. items.Select(item => LauncherRow.ForItem(section.ExtensionCapabilityId, item))]));
                if (_selection < 0)
                {
                    Move(1);
                }
            }

            if (stale && !cts.IsCancellationRequested)
            {
                // A resultless query shows no list at all.
                Reset();
            }
        }
        catch (Exception e) when (PalomaClient.IsCancellation(e))
        {
        }
        catch (Exception e)
        {
            Report($"Search failed: {PalomaClient.Describe(e)}");
        }
    }

    public async Task<RunActionResponse?> ActivateAsync(LauncherRow row, ExtAction action)
    {
        // A held Enter or a double click must not run the action twice.
        if (_activating)
        {
            return null;
        }

        _activating = true;
        try
        {
            RunActionResponse? behavior = null;
            await RpcGuard.TryAsync(
                async () => behavior = await client.RunSearchActionAsync(row.CapabilityId, action),
                Report,
                "Action failed");
            return behavior;
        }
        finally
        {
            _activating = false;
        }
    }

    public void Clear()
    {
        _searchCts?.Cancel();
        Reset();
    }

    /// <summary>Moves the flat selection and returns its new index; the ask
    /// row and an empty list both report -1.</summary>
    public int Move(int delta)
    {
        var count = RowCount;
        if (count == 0)
        {
            return _selection;
        }

        // The ask row sits one slot past the last result.
        var position = (AskSelected ? count : _selection) + delta;
        if (position < 0 || position > count)
        {
            return _selection;
        }

        // new select row is the ask row
        if (position == count)
        {
            if (SelectedRow is { } row)
            {
                row.IsSelected = false;
            }

            _selection = -1;
            AskSelected = true;
            return _selection;
        }

        AskSelected = false;
        Select(position);
        return _selection;
    }

    public void Select(LauncherRow row)
    {
        var index = 0;
        foreach (var candidate in Groups.SelectMany(group => group.Items))
        {
            if (candidate == row)
            {
                AskSelected = false;
                Select(index);
                return;
            }

            index++;
        }
    }

    public void Dispose()
    {
        _searchCts?.Dispose();
    }

    private void Select(int index)
    {
        if (SelectedRow is { } previous)
        {
            previous.IsSelected = false;
        }

        _selection = index;
        RowAt(index)!.IsSelected = true;
    }

    private LauncherRow? RowAt(int index)
    {
        return index >= 0
            ? Groups.SelectMany(group => group.Items).ElementAtOrDefault(index)
            : null;
    }

    private void Report(string message)
    {
        _messenger.Send(new ErrorReportedMessage(message));
    }

    private void Reset()
    {
        Groups.Clear();
        _selection = -1;
        AskSelected = false;
        HasResults = false;
    }
}

public sealed class SearchGroup(string name, IReadOnlyList<LauncherRow> items)
{
    public string Name { get; } = name;

    public IReadOnlyList<LauncherRow> Items { get; } = items;
}

public sealed partial class LauncherRow : ObservableObject
{
    public ExtensionCapabilityId CapabilityId { get; }

    public Item Item { get; }

    public ExtAction? PrimaryAction =>
        Item.Actions.FirstOrDefault(action => action.Primary) ?? Item.Actions.FirstOrDefault();

    public bool HasActionMenu => Item.Actions.Count > 1;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(ShowActionHint))]
    public partial bool IsSelected { get; set; }

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(ShowActionHint))]
    [NotifyPropertyChangedFor(nameof(ShowMoreButton))]
    public partial bool IsHovered { get; set; }

    [ObservableProperty] public partial IconElement? Icon { get; private set; }

    public bool ShowsIcon { get; private set; }

    public bool ShowActionHint => IsSelected && !IsHovered && HasActionMenu;

    public bool ShowMoreButton => IsHovered && HasActionMenu;

    private LauncherRow(ExtensionCapabilityId capabilityId, Item item)
    {
        CapabilityId = capabilityId;
        Item = item;
    }

    public static LauncherRow ForItem(ExtensionCapabilityId capabilityId, Item item)
    {
        var row = new LauncherRow(capabilityId, item);
        if (item.Icon is not { } icon || !CapabilityIcons.CanLoad(icon)) return row;
        // arbitrary icon requires image read and is blocking,
        // hence sideload the read process to not block the UI thread
        row.ShowsIcon = true;
        _ = row.LoadIconAsync(icon);

        return row;
    }

    private async Task LoadIconAsync(CapabilityIcon icon)
    {
        Icon = await CapabilityIcons.LoadAsync(icon);
    }
}