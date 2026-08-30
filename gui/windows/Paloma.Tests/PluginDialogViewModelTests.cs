using PalomaCore;
using Paloma.ViewModels.Settings;
using Xunit;

namespace Paloma.Tests;

public sealed class PluginDialogViewModelTests
{
    private static readonly string[] ExpectedLocalArgs = ["--flag", "value"];

    private static PluginDialogViewModel McpAdd(MockPalomaClient mock) =>
        new(mock, new HashSet<string>(), PluginType.Mcp, null);

    [Fact]
    public void EmptyForm_McpAdd_CannotSubmit()
    {
        Assert.False(McpAdd(new MockPalomaClient()).CanSubmit);
    }

    [Fact]
    public void TakenName_ReportsNameError()
    {
        var vm = new PluginDialogViewModel(
            new MockPalomaClient(), new HashSet<string> { "existing" }, PluginType.Mcp, null)
        {
            Name = "existing"
        };

        Assert.NotNull(vm.NameError);
        Assert.False(vm.CanSubmit);
    }

    [Fact]
    public void MalformedArgs_ReportsArgsError()
    {
        var vm = McpAdd(new MockPalomaClient());
        vm.Name = "server";
        vm.Command = "npx";

        vm.Args = "not json";

        Assert.NotNull(vm.ArgsError);
        Assert.False(vm.CanSubmit);

        vm.Args = "[\"--flag\"]";

        Assert.Null(vm.ArgsError);
        Assert.True(vm.CanSubmit);
    }

    [Fact]
    public void RemoteType_SwapsValidationToUrl()
    {
        var vm = McpAdd(new MockPalomaClient());
        vm.Name = "server";
        vm.TypeIndex = 1;

        Assert.True(vm.IsRemote);
        // An untouched URL blocks submit quietly, without an error message.
        Assert.Null(vm.UrlError);
        Assert.False(vm.CanSubmit);

        vm.Url = "ftp://example.com";

        Assert.NotNull(vm.UrlError);
        Assert.False(vm.CanSubmit);

        vm.Url = "https://example.com/mcp";

        Assert.Null(vm.UrlError);
        Assert.True(vm.CanSubmit);
    }

    [Fact]
    public void MalformedEnv_ReportsErrorAndExpandsAdvanced()
    {
        var vm = McpAdd(new MockPalomaClient());
        Assert.False(vm.AdvancedExpanded);

        vm.Env = "not json";

        Assert.NotNull(vm.EnvError);
        Assert.True(vm.AdvancedExpanded);
        Assert.False(vm.CanSubmit);

        vm.Env = "{\"KEY\": \"value\"}";

        Assert.Null(vm.EnvError);
    }

    [Fact]
    public void EditingRemote_PrefillsFormWithReadOnlyName()
    {
        var mock = new MockPalomaClient();
        var editing = new Plugin(
            "server",
            Transport.Http,
            60,
            false,
            new Dictionary<string, string> { ["KEY"] = "value" },
            new PluginArgs.Remote("https://example.com/mcp", true));

        var vm = new PluginDialogViewModel(
            mock, new HashSet<string>(), PluginType.Mcp, editing);

        Assert.False(vm.NameEditable);
        Assert.Equal(1, vm.TypeIndex);
        Assert.Equal("https://example.com/mcp", vm.Url);
        Assert.True(vm.RequiresAuth);
        Assert.Equal("{\"KEY\":\"value\"}", vm.Env);
        // Customized timeout and env surface the advanced section.
        Assert.True(vm.AdvancedExpanded);
        Assert.True(vm.CanSubmit);
    }

    [Fact]
    public async Task Submit_LocalMcp_FinalizesTheConnection()
    {
        var mock = new MockPalomaClient();
        var vm = McpAdd(mock);
        vm.Name = "server";
        vm.Command = "npx";
        vm.Args = "[\"--flag\", \"value\"]";

        Assert.True(await vm.SubmitAsync());

        var config = Assert.Single(mock.FinalizedMcps);
        Assert.Equal("server", config.Name);
        var local = Assert.IsType<PluginArgs.Local>(config.Args);
        Assert.Equal("npx", local.Command);
        Assert.Equal(ExpectedLocalArgs, local.Args);
    }

    [Fact]
    public async Task Submit_WhenEditing_RoutesToUpdate()
    {
        var mock = new MockPalomaClient();
        var editing = new Plugin(
            "server",
            Transport.Local,
            300,
            false,
            [],
            new PluginArgs.Local("npx", ["--flag"]));
        var vm = new PluginDialogViewModel(
            mock, new HashSet<string>(), PluginType.Mcp, editing);

        Assert.True(await vm.SubmitAsync());

        var (kind, config) = Assert.Single(mock.UpdatedPlugins);
        Assert.Equal(PluginType.Mcp, kind);
        Assert.Equal("server", config.Name);
        Assert.Empty(mock.FinalizedMcps);
    }

    [Fact]
    public async Task Submit_Failure_LandsInSubmitErrorAndFormStaysUp()
    {
        var mock = new MockPalomaClient
        {
            OnAddExtensionPlugin = _ =>
                throw new InvalidOperationException("core is down"),
        };
        var vm = new PluginDialogViewModel(
            mock, new HashSet<string>(), PluginType.Extension, null)
        {
            Command = @"C:\tools\extension.exe"
        };

        Assert.False(await vm.SubmitAsync());

        Assert.Contains("core is down", vm.SubmitError);
        Assert.True(vm.HasSubmitError);
        // The failed submit must hand the form back, not leave it disabled.
        Assert.True(vm.CanSubmit);
    }

    [Fact]
    public void Editing_OwnTakenNameIsNotAnError()
    {
        var editing = new Plugin(
            "server",
            Transport.Local,
            300,
            false,
            [],
            new PluginArgs.Local("npx", ["--flag"]));

        // The edited plugin's own name is always in the taken set; editing
        // must not flag it as a duplicate.
        var vm = new PluginDialogViewModel(
            new MockPalomaClient(), new HashSet<string> { "server" }, PluginType.Mcp, editing);

        Assert.Null(vm.NameError);
        Assert.True(vm.CanSubmit);
    }

    [Fact]
    public void EmptyArgs_McpBlocksSubmitQuietly()
    {
        var vm = McpAdd(new MockPalomaClient());
        vm.Name = "server";
        vm.Command = "npx";

        Assert.Null(vm.ArgsError);
        Assert.False(vm.CanSubmit);
    }

    [Fact]
    public void EmptyArgsArray_Mcp_ReportsArgsError()
    {
        var vm = McpAdd(new MockPalomaClient());
        vm.Name = "server";
        vm.Command = "npx";

        vm.Args = "[]";

        Assert.NotNull(vm.ArgsError);
        Assert.False(vm.CanSubmit);
    }

    [Fact]
    public void Extension_MalformedArgs_ReportsArgsError()
    {
        var vm = new PluginDialogViewModel(
            new MockPalomaClient(), new HashSet<string>(), PluginType.Extension, null)
        {
            Command = @"C:\tools\extension.exe"
        };
        Assert.True(vm.CanSubmit);

        vm.Args = "not json";

        Assert.NotNull(vm.ArgsError);
        Assert.False(vm.CanSubmit);
    }

    [Fact]
    public void TimeoutOutOfRange_BlocksSubmit()
    {
        var vm = McpAdd(new MockPalomaClient());
        vm.Name = "server";
        vm.Command = "npx";
        vm.Args = "[\"--flag\"]";
        Assert.True(vm.CanSubmit);

        vm.Timeout = 0;
        Assert.False(vm.CanSubmit);

        vm.Timeout = double.NaN;
        Assert.False(vm.CanSubmit);

        vm.Timeout = 300;
        Assert.True(vm.CanSubmit);
    }

    [Fact]
    public void SwitchingBackToLocal_ClearsTheUrlError()
    {
        var vm = McpAdd(new MockPalomaClient());
        vm.Name = "server";
        vm.TypeIndex = 1;
        vm.Url = "ftp://example.com";
        Assert.NotNull(vm.UrlError);

        vm.TypeIndex = 0;

        Assert.Null(vm.UrlError);
    }
}