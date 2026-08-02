using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using Paloma.Binding.V1;
using Paloma.Client;
using Paloma.Helpers;

namespace Paloma.ViewModels.Settings;

public sealed partial class PluginsViewModel(IPalomaClient client) : ObservableObject
{
    public ObservableCollection<ExtensionViewModel> Extensions { get; } = [];

    public ObservableCollection<ProviderViewModel> Providers { get; } = [];

    public ObservableCollection<McpViewModel> Mcps { get; } = [];

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasStatus))]
    public partial string Status { get; set; } = string.Empty;

    public bool HasStatus => Status.Length > 0;

    [ObservableProperty] public partial bool HasMcps { get; private set; }

    public async Task LoadAsync()
    {
        if (await RpcGuard.TryAsync(
            async () =>
            {
                Fill(
                    Extensions,
                    await client.GetExtensionPluginsAsync(),
                    extension => new ExtensionViewModel(client, extension, Report));
                Fill(
                    Providers,
                    await client.GetProviderPluginsAsync(),
                    provider => new ProviderViewModel(client, provider, Report));
                Fill(
                    Mcps,
                    await client.GetMcpsAsync(),
                    mcp => new McpViewModel(client, mcp, Report));
                HasMcps = Mcps.Count > 0;
            },
            message => Status = message,
            "Failed to load plugins"))
        {
            Status = string.Empty;
        }
    }

    public HashSet<string> TakenNames() =>
        [.. Extensions.Concat<PluginViewModel>(Providers).Concat(Mcps).Select(plugin => plugin.Name)];

    public async Task RemoveAsync(PluginViewModel plugin)
    {
        string? failure = null;
        await RpcGuard.TryAsync(
            () => client.RemovePluginAsync(plugin.Kind, plugin.Name),
            message => failure = message,
            $"Failed to remove {plugin.Name}");
        await LoadAsync();
        if (failure is not null)
        {
            // The reload wipes Status on success; the failure must outlive
            // it — the plugin is still in the list with no other explanation.
            Status = failure;
        }
    }

    private void Report(string message) => Status = message;

    private static void Fill<TInfo, TRow>(
        ObservableCollection<TRow> target,
        IReadOnlyList<TInfo> plugins,
        Func<TInfo, TRow> create)
    {
        target.Clear();
        foreach (var plugin in plugins)
        {
            target.Add(create(plugin));
        }
    }
}

public abstract partial class PluginViewModel : ObservableObject
{
    private readonly ToggleGuard _toggle;

    public string Name { get; }

    public string Description { get; }

    public string HealthLabel { get; }

    public string? Error { get; }

    public Plugin? Config { get; }

    // Built-ins ship no config; they can be neither managed nor disabled.
    public bool CanManage => Config is not null;

    public PluginType Kind { get; }

    [ObservableProperty] public partial bool Enabled { get; set; }

    private protected PluginViewModel(
        IPalomaClient client,
        PluginType kind,
        string name,
        string description,
        HealthStatus health,
        string? error,
        Plugin? config,
        Action<string> report)
    {
        Kind = kind;
        Name = name;
        Description = description;
        _toggle = new ToggleGuard(
            name,
            value => client.TogglePluginAsync(name, disabled: !value),
            value => Enabled = value,
            report);
        // Healthy is the norm and stays quiet; only exceptional states
        // label themselves.
        HealthLabel = health switch
        {
            HealthStatus.Starting => "Starting",
            HealthStatus.Unhealthy => "Unhealthy",
            _ => string.Empty,
        };
        Error = error;
        Config = config;
        Enabled = !(config?.Disabled ?? false);
        _toggle.Ready();
    }

    private protected static List<CapabilityToggleViewModel> Toggles(
        IPalomaClient client,
        string plugin,
        IEnumerable<CapabilityInfo> capabilities,
        CapabilityFacet facet,
        Action<string> report) =>
    [
        .. capabilities
            .Where(capability => capability.Facets.Any(state => state.Facet == facet))
            .Select(capability => new CapabilityToggleViewModel(
                client,
                plugin,
                capability,
                facet,
                report))
    ];

    partial void OnEnabledChanged(bool value) => _toggle.Changed(value);
}

public sealed partial class ExtensionViewModel(IPalomaClient client, ExtensionInfo extension, Action<string> report)
    : PluginViewModel(
        client,
        PluginType.Extension,
        extension.Name,
        extension.Description,
        extension.Status,
        extension.HasError ? extension.Error : null,
        extension.Config,
        report)
{
    public IReadOnlyList<CapabilityToggleViewModel> SearchCapabilities { get; } =
        Toggles(client, extension.Name, extension.Capabilities, CapabilityFacet.Search, report);

    public IReadOnlyList<CapabilityToggleViewModel> ToolCapabilities { get; } =
        Toggles(client, extension.Name, extension.Capabilities, CapabilityFacet.Tool, report);

    public bool HasSearchCapabilities => SearchCapabilities.Count > 0;

    public bool HasToolCapabilities => ToolCapabilities.Count > 0;

    public bool HasCapabilities => HasSearchCapabilities || HasToolCapabilities;
}

public sealed partial class ProviderViewModel(IPalomaClient client, ProviderInfo provider, Action<string> report)
    : PluginViewModel(
        client,
        PluginType.Provider,
        provider.Name,
        provider.Description,
        provider.Status,
        provider.HasError ? provider.Error : null,
        provider.Config,
        report);

public sealed partial class McpViewModel(IPalomaClient client, McpPluginInfo mcp, Action<string> report)
    : PluginViewModel(
        client,
        PluginType.Mcp,
        mcp.Config?.Name ?? string.Empty,
        mcp.Description,
        mcp.Status,
        mcp.HasError ? mcp.Error : null,
        mcp.Config,
        report)
{
    public IReadOnlyList<CapabilityToggleViewModel> Tools { get; } =
        Toggles(client, mcp.Config?.Name ?? string.Empty, mcp.Tools, CapabilityFacet.Mcp, report);

    public bool HasTools => Tools.Count > 0;
}

public sealed partial class CapabilityToggleViewModel : ObservableObject
{
    private readonly ToggleGuard _toggle;

    public string Id { get; }

    public string Description { get; }

    [ObservableProperty] public partial bool Enabled { get; set; }

    public CapabilityToggleViewModel(
        IPalomaClient client,
        string plugin,
        CapabilityInfo capability,
        CapabilityFacet facet,
        Action<string> report)
    {
        Id = capability.Id;
        Description = capability.Description;
        _toggle = new ToggleGuard(
            capability.Id,
            value => client.ToggleCapabilityAsync(plugin, capability.Id, facet, disabled: !value),
            value => Enabled = value,
            report);
        Enabled = !capability.Facets.Any(state => state.Facet == facet && state.Disabled);
        _toggle.Ready();
    }

    partial void OnEnabledChanged(bool value) => _toggle.Changed(value);
}
