using PalomaCore;
using Paloma.ViewModels.Settings;
using Xunit;

namespace Paloma.Tests;

public sealed class PluginsViewModelTests
{
    private static CapabilityInfo Capability(string id, params CapabilityFacet[] facets)
    {
        return new CapabilityInfo(id, $"{id} capability", [.. facets.Select(facet => new FacetState(facet, false))]);
    }

    private static ExtensionInfo EnabledExtension() => new(
        "my-extension",
        "a test extension",
        null,
        null,
        [Capability("cap", CapabilityFacet.Search)],
        HealthStatus.Running,
        null,
        TestProtos.LocalPlugin("my-extension", "cmd"));

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
                throw new InvalidOperationException("storage failure"),
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
                throw new InvalidOperationException("core away"),
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
        var extension = EnabledExtension() with
        {
            Capabilities =
            [
                Capability("search-only", CapabilityFacet.Search),
                Capability("tool-only", CapabilityFacet.Tool),
                Capability("both", CapabilityFacet.Search, CapabilityFacet.Tool),
            ],
        };
        var vm = new PluginsViewModel(new MockPalomaClient { ExtensionPlugins = [extension] });

        await vm.LoadAsync();

        var row = Assert.Single(vm.Extensions);
        Assert.Equal(["search-only", "both"], row.SearchCapabilities.Select(c => c.Id));
        Assert.Equal(["tool-only", "both"], row.ToolCapabilities.Select(c => c.Id));
    }

    [Fact]
    public async Task Mcp_TakesNameFromConfigAndFiltersToolsByMcpFacet()
    {
        var mcp = new McpPluginInfo(
            "a test server",
            HealthStatus.Running,
            null,
            [Capability("tool", CapabilityFacet.Mcp), Capability("not-a-tool", CapabilityFacet.Search)],
            TestProtos.LocalPlugin("server"));
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