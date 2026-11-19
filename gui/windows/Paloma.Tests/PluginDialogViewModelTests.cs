using PalomaCore;
using Paloma.ViewModels.Settings;
using Xunit;

namespace Paloma.Tests;

public sealed class PluginDialogViewModelTests
{
    private static readonly string[] ExpectedLocalArgs = ["--flag", "value"];
    private static readonly string[] SortedHeaderNames = ["Accept", "X-Tenant"];

    private static PluginDialogViewModel McpAdd(MockPalomaClient mock) =>
        new(mock, new HashSet<string>(), PluginType.Mcp, null);

    private static PluginDialogViewModel RemoteAdd(MockPalomaClient mock)
    {
        var vm = McpAdd(mock);
        vm.Name = "server";
        vm.TypeIndex = 1;
        vm.Url = "https://example.com/mcp";
        return vm;
    }

    private static PluginDialogViewModel RemoteEdit(RemoteAuth auth) =>
        new(
            new MockPalomaClient(),
            new HashSet<string>(),
            PluginType.Mcp,
            new Plugin(
                "server",
                Transport.Http,
                300,
                false,
                [],
                new PluginArgs.Remote("https://example.com/mcp", auth)));

    private static void Select<TAuth>(PluginDialogViewModel vm) where TAuth : RemoteAuth =>
        vm.Auth = PluginDialogViewModel.AuthOptions.Single(option => option.Kind is TAuth);

    private static async Task<RemoteAuth> SubmittedAuthAsync(MockPalomaClient mock, PluginDialogViewModel vm)
    {
        Assert.True(await vm.SubmitAsync());
        var remote = Assert.IsType<PluginArgs.Remote>(Assert.Single(mock.FinalizedMcps).Args);
        return remote.Auth;
    }

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
    public void DuplicateEnvNames_AreFlaggedBlockSubmitAndExpandAdvanced()
    {
        var vm = McpAdd(new MockPalomaClient());
        vm.Name = "server";
        vm.Command = "npx";
        vm.Args = "[\"--flag\"]";
        Assert.Empty(vm.Env);
        Assert.False(vm.AdvancedExpanded);
        Assert.True(vm.CanSubmit);

        vm.AddVariable();
        vm.Env[0].Name = "API_KEY";
        vm.AddVariable();
        vm.Env[1].Name = " api_key ";

        Assert.True(vm.Env[0].IsDuplicate);
        Assert.True(vm.Env[1].IsDuplicate);
        Assert.True(vm.AdvancedExpanded);
        Assert.False(vm.CanSubmit);

        vm.Env[1].Remove();

        Assert.False(Assert.Single(vm.Env).IsDuplicate);
        Assert.True(vm.CanSubmit);
    }

    [Fact]
    public async Task Submit_SendsTheTrimmedEnvWithoutBlankNames()
    {
        var mock = new MockPalomaClient();
        var vm = McpAdd(mock);
        vm.Name = "server";
        vm.Command = "npx";
        vm.Args = "[\"--flag\"]";
        vm.AddVariable();
        vm.Env[0].Name = " API_KEY ";
        vm.Env[0].Value = " secret ";
        vm.AddVariable();
        vm.Env[1].Value = "orphan";

        Assert.True(await vm.SubmitAsync());

        var (name, value) = Assert.Single(Assert.Single(mock.FinalizedMcps).Env);
        Assert.Equal("API_KEY", name);
        Assert.Equal("secret", value);
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
            new PluginArgs.Remote("https://example.com/mcp", new RemoteAuth.Oauth()));

        var vm = new PluginDialogViewModel(
            mock, new HashSet<string>(), PluginType.Mcp, editing);

        Assert.False(vm.NameEditable);
        Assert.Equal(1, vm.TypeIndex);
        Assert.Equal("https://example.com/mcp", vm.Url);
        Assert.IsType<RemoteAuth.Oauth>(vm.Auth.Kind);
        var variable = Assert.Single(vm.Env);
        Assert.Equal("KEY", variable.Name);
        Assert.Equal("value", variable.Value);
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
    public async Task Submit_Remote_DefaultsToNoAuth()
    {
        var mock = new MockPalomaClient();

        Assert.IsType<RemoteAuth.None>(await SubmittedAuthAsync(mock, RemoteAdd(mock)));
    }

    [Fact]
    public async Task Submit_RemoteWithOauth_SendsOauth()
    {
        var mock = new MockPalomaClient();
        var vm = RemoteAdd(mock);
        Select<RemoteAuth.Oauth>(vm);

        Assert.IsType<RemoteAuth.Oauth>(await SubmittedAuthAsync(mock, vm));
    }

    [Fact]
    public async Task Submit_RemoteWithBearerToken_SendsTheTrimmedToken()
    {
        var mock = new MockPalomaClient();
        var vm = RemoteAdd(mock);
        Select<RemoteAuth.Bearer>(vm);
        Assert.False(vm.CanSubmit);

        vm.Token = " secret ";

        Assert.True(vm.CanSubmit);
        Assert.Equal(new RemoteAuth.Bearer("secret"), await SubmittedAuthAsync(mock, vm));
    }

    [Fact]
    public async Task Submit_RemoteWithOauthClient_SendsTheTrimmedClientId()
    {
        var mock = new MockPalomaClient();
        var vm = RemoteAdd(mock);
        Select<RemoteAuth.OauthClient>(vm);
        Assert.False(vm.CanSubmit);

        vm.ClientId = " paloma-desktop ";

        Assert.True(vm.CanSubmit);
        Assert.Equal(new RemoteAuth.OauthClient("paloma-desktop"), await SubmittedAuthAsync(mock, vm));
    }

    [Fact]
    public async Task Submit_RemoteWithHeaders_SendsTheTrimmedPairsWithoutBlankNames()
    {
        var mock = new MockPalomaClient();
        var vm = RemoteAdd(mock);
        Select<RemoteAuth.Headers>(vm);
        Assert.True(vm.CanSubmit);

        var first = Assert.Single(vm.Headers);
        first.Name = " X-API-Key ";
        first.Value = " key-456 ";
        vm.AddHeader();
        vm.Headers[1].Value = "orphan";

        var headers = Assert.IsType<RemoteAuth.Headers>(await SubmittedAuthAsync(mock, vm));
        var (name, value) = Assert.Single(headers.HeadersValue);
        Assert.Equal("X-API-Key", name);
        Assert.Equal("key-456", value);
    }

    [Fact]
    public void DuplicateHeaderNames_AreFlaggedAndBlockSubmit()
    {
        var vm = RemoteAdd(new MockPalomaClient());
        Select<RemoteAuth.Headers>(vm);
        vm.Headers[0].Name = "X-API-Key";
        vm.AddHeader();
        vm.Headers[1].Name = "Accept";
        vm.AddHeader();

        vm.Headers[2].Name = " x-api-key ";

        Assert.True(vm.Headers[0].IsDuplicate);
        Assert.False(vm.Headers[1].IsDuplicate);
        Assert.True(vm.Headers[2].IsDuplicate);
        Assert.False(vm.CanSubmit);

        vm.Headers[2].Name = "X-Tenant";

        Assert.DoesNotContain(vm.Headers, row => row.IsDuplicate);
        Assert.True(vm.CanSubmit);
    }

    [Fact]
    public void RemovingADuplicateHeader_ClearsTheFlag()
    {
        var vm = RemoteAdd(new MockPalomaClient());
        Select<RemoteAuth.Headers>(vm);
        vm.Headers[0].Name = "X-API-Key";
        vm.AddHeader();
        vm.Headers[1].Name = "X-API-Key";
        Assert.False(vm.CanSubmit);

        vm.Headers[1].Remove();

        Assert.False(Assert.Single(vm.Headers).IsDuplicate);
        Assert.True(vm.CanSubmit);
    }

    [Fact]
    public void DuplicateHeaderNames_DoNotBlockAnotherAuth()
    {
        var vm = RemoteAdd(new MockPalomaClient());
        Select<RemoteAuth.Headers>(vm);
        vm.Headers[0].Name = "X-API-Key";
        vm.AddHeader();
        vm.Headers[1].Name = "X-API-Key";
        Assert.False(vm.CanSubmit);

        Select<RemoteAuth.Oauth>(vm);

        Assert.True(vm.CanSubmit);
    }

    [Fact]
    public void EditingRemote_PrefillsTheSavedAuth()
    {
        var bearer = RemoteEdit(new RemoteAuth.Bearer("secret"));
        Assert.IsType<RemoteAuth.Bearer>(bearer.Auth.Kind);
        Assert.Equal("secret", bearer.Token);
        Assert.True(bearer.CanSubmit);

        var client = RemoteEdit(new RemoteAuth.OauthClient("paloma-desktop"));
        Assert.IsType<RemoteAuth.OauthClient>(client.Auth.Kind);
        Assert.Equal("paloma-desktop", client.ClientId);
        Assert.True(client.CanSubmit);

        var headers = RemoteEdit(new RemoteAuth.Headers(
            new Dictionary<string, string> { ["X-Tenant"] = "acme", ["Accept"] = "application/json" }));
        Assert.IsType<RemoteAuth.Headers>(headers.Auth.Kind);
        Assert.Equal(SortedHeaderNames, headers.Headers.Select(row => row.Name));
        Assert.Equal("application/json", headers.Headers[0].Value);
        Assert.True(headers.CanSubmit);

        var empty = RemoteEdit(new RemoteAuth.Headers([]));
        Assert.IsType<RemoteAuth.Headers>(empty.Auth.Kind);
        Assert.Equal(string.Empty, Assert.Single(empty.Headers).Name);
        Assert.True(empty.CanSubmit);
    }

    [Fact]
    public void SwitchingBackToLocal_IgnoresUnfinishedAuth()
    {
        var vm = RemoteAdd(new MockPalomaClient());
        vm.Command = "npx";
        vm.Args = "[\"--flag\"]";
        Select<RemoteAuth.Bearer>(vm);
        Assert.False(vm.CanSubmit);

        vm.TypeIndex = 0;

        Assert.True(vm.CanSubmit);
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