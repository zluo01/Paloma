using Grpc.Core;
using Paloma.Binding.V1;
using Paloma.ViewModels.Settings;
using Xunit;

namespace Paloma.Tests;

public sealed class PluginsViewModelTests
{
    private static CapabilityInfo Capability(string id, params CapabilityFacet[] facets)
    {
        var capability = new CapabilityInfo { Id = id, Description = $"{id} capability" };
        foreach (var facet in facets)
        {
            capability.Facets.Add(new FacetState { Facet = facet });
        }
        return capability;
    }

    private static ExtensionInfo EnabledExtension() => new()
    {
        Name = "my-extension",
        Description = "a test extension",
        Status = HealthStatus.Running,
        Capabilities = { Capability("cap", CapabilityFacet.Search) },
        Config = new Plugin
        {
            Name = "my-extension",
            Timeout = 300,
            Local = new LocalPluginArgs { Command = "cmd" },
        },
    };

    [Fact]
    public async Task Load_DoesNotWriteTogglesBack()
    {
        var mock = new MockPalomaClient { ExtensionPlugins = [EnabledExtension()] };
        var vm = new PluginsViewModel(mock);

        await vm.LoadAsync();

        // Viewing the plugins page must be read-only: no plugin or
        // capability toggle writes may fire from initialization.
        Assert.Empty(mock.PluginToggles);
        Assert.Empty(mock.CapabilityToggles);
    }

    [Fact]
    public async Task Remove_WhenRpcFails_KeepsTheFailureStatusAfterReload()
    {
        var mock = new MockPalomaClient
        {
            ExtensionPlugins = [EnabledExtension()],
            OnRemovePlugin = (_, _) =>
                throw new RpcException(new Status(StatusCode.Internal, "storage failure")),
        };
        var vm = new PluginsViewModel(mock);
        await vm.LoadAsync();

        await vm.RemoveAsync(vm.Extensions[0]);

        // The reload wipes Status on success; the failure the user needs to
        // see must outlive it - the plugin is still in the list.
        Assert.Contains("storage failure", vm.Status);
    }

    [Fact]
    public async Task Toggle_WhenRpcFails_RevertsTheSwitch()
    {
        var mock = new MockPalomaClient
        {
            ExtensionPlugins = [EnabledExtension()],
            OnTogglePlugin = (_, _) =>
                throw new RpcException(new Status(StatusCode.Unavailable, "core away")),
        };
        var vm = new PluginsViewModel(mock);
        await vm.LoadAsync();
        var plugin = Assert.Single(vm.Extensions);

        plugin.Enabled = false;

        // The write failed, so the switch must not keep showing a state
        // that was never persisted.
        await TestWait.UntilAsync(() => plugin.Enabled);
    }

    [Fact]
    public async Task Extension_RoutesCapabilitiesByFacet()
    {
        var extension = EnabledExtension();
        extension.Capabilities.Clear();
        extension.Capabilities.Add(Capability("search-only", CapabilityFacet.Search));
        extension.Capabilities.Add(Capability("tool-only", CapabilityFacet.Tool));
        extension.Capabilities.Add(Capability("both", CapabilityFacet.Search, CapabilityFacet.Tool));
        var vm = new PluginsViewModel(new MockPalomaClient { ExtensionPlugins = [extension] });

        await vm.LoadAsync();

        var row = Assert.Single(vm.Extensions);
        Assert.Equal(["search-only", "both"], row.SearchCapabilities.Select(c => c.Id));
        Assert.Equal(["tool-only", "both"], row.ToolCapabilities.Select(c => c.Id));
    }

    [Fact]
    public async Task Mcp_TakesNameFromConfigAndFiltersToolsByMcpFacet()
    {
        var mcp = new McpPluginInfo
        {
            Description = "a test server",
            Status = HealthStatus.Running,
            Tools =
            {
                Capability("tool", CapabilityFacet.Mcp),
                Capability("not-a-tool", CapabilityFacet.Search),
            },
            Config = new Plugin { Name = "server" },
        };
        var vm = new PluginsViewModel(new MockPalomaClient { McpPlugins = [mcp] });

        await vm.LoadAsync();

        var row = Assert.Single(vm.Mcps);
        Assert.Equal("server", row.Name);
        Assert.Equal("tool", Assert.Single(row.Tools).Id);
    }

    [Fact]
    public async Task CapabilityToggle_PersistsWithItsFacet()
    {
        var mock = new MockPalomaClient { ExtensionPlugins = [EnabledExtension()] };
        var vm = new PluginsViewModel(mock);
        await vm.LoadAsync();
        var capability = Assert.Single(vm.Extensions[0].SearchCapabilities);

        capability.Enabled = false;

        await TestWait.UntilAsync(() => mock.CapabilityToggles.Count == 1);
        Assert.Equal(
            ("my-extension", "cap", CapabilityFacet.Search, true),
            mock.CapabilityToggles[0]);
    }

    [Fact]
    public async Task Remove_PassesTheKindAndName()
    {
        (PluginType Kind, string Name)? removed = null;
        var mock = new MockPalomaClient
        {
            ExtensionPlugins = [EnabledExtension()],
            OnRemovePlugin = (kind, name) => removed = (kind, name),
        };
        var vm = new PluginsViewModel(mock);
        await vm.LoadAsync();

        await vm.RemoveAsync(vm.Extensions[0]);

        Assert.Equal((PluginType.Extension, "my-extension"), removed);
    }
}
