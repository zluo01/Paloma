using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using Paloma.Client;
using Paloma.Helpers;
using Permission = PalomaCore.Permission;

namespace Paloma.ViewModels.Settings;

public sealed partial class PermissionsViewModel(IPalomaClient client) : ObservableObject
{
    private IReadOnlyList<Permission> _all = [];

    public ObservableCollection<Permission> Permissions { get; } = [];

    [ObservableProperty] public partial string Status { get; private set; } = string.Empty;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasError))]
    public partial string Error { get; private set; } = string.Empty;

    [ObservableProperty] public partial string Filter { get; set; } = string.Empty;

    public bool HasError => Error.Length > 0;

    partial void OnFilterChanged(string value) => Apply();

    public async Task LoadAsync()
    {
        if (await ClientGuard.TryAsync(
                async () =>
                {
                    _all = await client.GetPermissionsAsync();
                    Apply();
                },
                message => Error = message,
                "Failed to load permissions"))
        {
            Error = string.Empty;
        }
    }

    public async Task DeleteAsync(Permission permission)
    {
        if (await ClientGuard.TryAsync(
                async () =>
                {
                    await client.DeletePermissionAsync(permission.Prefix);
                    _all = [.. _all.Where(p => p.Prefix != permission.Prefix)];
                    Apply();
                },
                message => Error = message,
                "Failed to delete permission"))
        {
            Error = string.Empty;
        }
    }

    private void Apply()
    {
        var needle = Filter.Trim();
        var visible = needle.Length == 0
            ? _all
            :
            [
                .. _all
                    .Where(permission =>
                        permission.Prefix.Contains(needle, StringComparison.OrdinalIgnoreCase))
            ];

        Permissions.Clear();
        foreach (var permission in visible)
        {
            Permissions.Add(permission);
        }

        Status = (_all.Count, visible.Count) switch
        {
            (0, _) => "No saved permissions.",
            (_, 0) => "No permissions match the search.",
            _ => string.Empty,
        };
    }
}