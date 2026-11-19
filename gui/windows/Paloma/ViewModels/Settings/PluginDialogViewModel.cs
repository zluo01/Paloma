using System.Collections.ObjectModel;
using System.Text.Json;
using CommunityToolkit.Mvvm.ComponentModel;
using PalomaCore;
using Paloma.Client;
using Paloma.Helpers;

namespace Paloma.ViewModels.Settings;

public sealed partial class PluginDialogViewModel : ObservableObject, IDisposable
{
    public static IReadOnlyList<AuthOption> AuthOptions { get; } =
    [
        new(new RemoteAuth.None(), "None"),
        new(new RemoteAuth.Bearer(string.Empty), "Bearer token"),
        new(new RemoteAuth.Headers([]), "Custom headers"),
        new(new RemoteAuth.Oauth(), "OAuth"),
        new(new RemoteAuth.OauthClient(string.Empty), "OAuth with client ID"),
    ];

    // Mirrors core's stored default (db/queries/init_table.sql: timeout
    // DEFAULT 300); the customized-settings detection breaks silently if
    // they drift.
    private const uint CoreDefaultTimeout = 300;

    public string Title { get; }

    public string PrimaryLabel { get; }

    public string SubmitErrorTitle { get; }

    public string CommandPlaceholder { get; }

    public bool NameEditable { get; }

    public bool IsMcp => _kind == PluginType.Mcp;

    [ObservableProperty] public partial string Name { get; set; } = string.Empty;

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsRemote))]
    [NotifyPropertyChangedFor(nameof(IsLocal))]
    public partial int TypeIndex { get; set; }

    // Index 1 is the Remote radio; the non-MCP kinds never show the choice
    // and keep the local fields.
    public bool IsRemote => IsMcp && TypeIndex == 1;

    public bool IsLocal => !IsRemote;

    [ObservableProperty] public partial string Command { get; set; } = string.Empty;

    [ObservableProperty] public partial string Args { get; set; } = string.Empty;

    [ObservableProperty] public partial string? ArgsError { get; private set; }

    [ObservableProperty] public partial string Url { get; set; } = string.Empty;

    [ObservableProperty] public partial string? UrlError { get; private set; }

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(IsBearerAuth))]
    [NotifyPropertyChangedFor(nameof(IsHeadersAuth))]
    [NotifyPropertyChangedFor(nameof(IsOauthClientAuth))]
    public partial AuthOption Auth { get; set; } = AuthOptions.Single(option => option.Kind is RemoteAuth.None);

    public bool IsBearerAuth => Auth.Kind is RemoteAuth.Bearer;

    public bool IsHeadersAuth => Auth.Kind is RemoteAuth.Headers;

    public bool IsOauthClientAuth => Auth.Kind is RemoteAuth.OauthClient;

    [ObservableProperty] public partial string Token { get; set; } = string.Empty;

    public ObservableCollection<KeyValueRow> Headers { get; } = [];

    [ObservableProperty] public partial string ClientId { get; set; } = string.Empty;

    [ObservableProperty] public partial double Timeout { get; set; } = CoreDefaultTimeout;

    public ObservableCollection<KeyValueRow> Env { get; } = [];

    [ObservableProperty] public partial bool AdvancedExpanded { get; set; }

    [ObservableProperty] public partial string? NameError { get; private set; }

    [ObservableProperty]
    [NotifyPropertyChangedFor(nameof(HasSubmitError))]
    public partial string SubmitError { get; private set; } = string.Empty;

    public bool HasSubmitError => SubmitError.Length > 0;

    // True while an MCP connect waits on browser authorization; the markup
    // swaps the form for the waiting panel on it.
    [ObservableProperty] public partial bool Authorizing { get; private set; }

    [ObservableProperty] public partial Uri? AuthUri { get; private set; }

    [ObservableProperty] public partial bool CanSubmit { get; private set; }

    private readonly IPalomaClient _client;
    private readonly IReadOnlySet<string> _takenNames;
    private readonly PluginType _kind;
    private readonly Plugin? _editing;
    private CancellationTokenSource? _finalize;
    private bool _submitting;

    public PluginDialogViewModel(
        IPalomaClient client,
        IReadOnlySet<string> takenNames,
        PluginType kind,
        Plugin? editing)
    {
        _client = client;
        _takenNames = takenNames;
        _kind = kind;
        _editing = editing;
        var noun = kind switch
        {
            PluginType.Extension => "Extension",
            PluginType.Provider => "Provider",
            PluginType.Mcp => "MCP Server",
            _ => throw new ArgumentOutOfRangeException(nameof(kind), kind, null)
        };
        Title = editing is null ? $"Add {noun}" : $"Edit {noun}";
        PrimaryLabel = editing is null ? "Add" : "Save";
        SubmitErrorTitle = editing is null ? $"Couldn't add the {noun}" : $"Couldn't save the {noun}";
        CommandPlaceholder = kind switch
        {
            PluginType.Extension => @"C:\path\to\extension.exe",
            PluginType.Provider => @"C:\path\to\provider.exe",
            PluginType.Mcp => "npx",
            _ => throw new ArgumentOutOfRangeException(nameof(kind), kind, null)
        };
        NameEditable = editing is null;
        Prefill();
        if (Headers.Count == 0)
        {
            AddHeader();
        }

        Validate();
    }

    private void Prefill()
    {
        if (_editing is null)
        {
            return;
        }

        Name = _editing.Name;
        Timeout = _editing.Timeout;
        Fill(Env, _editing.Env);
        // Customized advanced settings deserve to be seen while editing.
        AdvancedExpanded = _editing.Env.Count > 0 || _editing.Timeout != CoreDefaultTimeout;
        switch (_editing.Args)
        {
            case PluginArgs.Local local:
                TypeIndex = 0;
                Command = local.Command;
                Args = JsonSerializer.Serialize(local.Args);
                break;
            case PluginArgs.Remote remote:
                TypeIndex = 1;
                Url = remote.Url;
                PrefillAuth(remote.Auth);
                break;
        }
    }

    private void PrefillAuth(RemoteAuth auth)
    {
        Auth = AuthOptions.Single(option => option.Kind.GetType() == auth.GetType());
        switch (auth)
        {
            case RemoteAuth.Bearer bearer:
                Token = bearer.Token;
                break;
            case RemoteAuth.Headers headers:
                Fill(Headers, headers.HeadersValue);
                break;
            case RemoteAuth.OauthClient client:
                ClientId = client.ClientId;
                break;
        }
    }

    partial void OnNameChanged(string value) => Validate();

    partial void OnTypeIndexChanged(int value) => Validate();

    partial void OnCommandChanged(string value) => Validate();

    partial void OnArgsChanged(string value) => Validate();

    partial void OnUrlChanged(string value) => Validate();

    partial void OnAuthChanged(AuthOption value) => Validate();

    partial void OnTokenChanged(string value) => Validate();

    partial void OnClientIdChanged(string value) => Validate();

    partial void OnTimeoutChanged(double value) => Validate();

    private static List<string>? ParseArgs(string text)
    {
        try
        {
            return JsonSerializer.Deserialize<List<string>>(text);
        }
        catch (JsonException)
        {
            return null;
        }
    }

    public void AddHeader() => AddRow(Headers);

    public void AddVariable() => AddRow(Env);

    private void AddRow(ObservableCollection<KeyValueRow> rows, string name = "", string value = "")
    {
        rows.Add(new KeyValueRow(Validate, row => RemoveRow(rows, row)) { Name = name, Value = value });
    }

    private void RemoveRow(ObservableCollection<KeyValueRow> rows, KeyValueRow row)
    {
        rows.Remove(row);
        Validate();
    }

    private void Fill(ObservableCollection<KeyValueRow> rows, Dictionary<string, string> values)
    {
        foreach (var (name, value) in values.OrderBy(pair => pair.Key, StringComparer.OrdinalIgnoreCase))
        {
            AddRow(rows, name, value);
        }
    }

    private static Dictionary<string, string>? Pairs(IEnumerable<KeyValueRow> rows)
    {
        var pairs = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        foreach (var row in rows)
        {
            var name = row.Name.Trim();
            if (name.Length > 0 && !pairs.TryAdd(name, row.Value.Trim()))
            {
                return null;
            }
        }

        return pairs;
    }

    /**
     * ENV variable and Headers are both case-insentive on windows.
     */
    private static void FlagDuplicates(IReadOnlyCollection<KeyValueRow> rows)
    {
        foreach (var row in rows)
        {
            var name = row.Name.Trim();
            row.IsDuplicate = name.Length > 0 && rows.Count(other =>
                string.Equals(other.Name.Trim(), name, StringComparison.OrdinalIgnoreCase)) > 1;
        }
    }

    private void Validate()
    {
        var valid = true;

        string? nameError = null;
        if (IsMcp && _editing is null)
        {
            var name = Name.Trim();
            if (name.Length == 0)
            {
                valid = false;
            }
            else if (_takenNames.Contains(name))
            {
                nameError = "A plugin with this name already exists.";
                valid = false;
            }
        }

        NameError = nameError;

        string? argsError = null;
        if (!IsRemote)
        {
            if (Command.Trim().Length == 0)
            {
                valid = false;
            }

            var argsText = Args.Trim();
            if (argsText.Length == 0)
            {
                if (IsMcp)
                {
                    valid = false;
                }
            }
            else if (ParseArgs(argsText) is not { } args || (IsMcp && args.Count == 0))
            {
                argsError = IsMcp
                    ? "Must be a non-empty JSON array like [\"--flag\", \"value\"]."
                    : "Must be a JSON array like [\"--log-level\", \"info\"].";
                valid = false;
            }
        }

        ArgsError = argsError;

        string? urlError = null;
        if (IsRemote)
        {
            var url = Url.Trim();
            if (url.Length == 0)
            {
                valid = false;
            }
            else if (!ValidUrl(url))
            {
                urlError = "Must be a valid http(s) URL.";
                valid = false;
            }

            FlagDuplicates(Headers);
            if (BuildAuth() is null)
            {
                valid = false;
            }
        }

        UrlError = urlError;

        FlagDuplicates(Env);
        if (Pairs(Env) is null)
        {
            valid = false;
            // A collapsed expander must not swallow its own error.
            AdvancedExpanded = true;
        }

        if (IsMcp && (double.IsNaN(Timeout) || Timeout is < 1 or > 3600))
        {
            valid = false;
        }

        CanSubmit = valid && !_submitting;
    }

    private static bool ValidUrl(string text)
    {
        return Uri.TryCreate(text, UriKind.Absolute, out var uri)
               && uri.Scheme is "http" or "https"
               && uri.Host.Length > 0;
    }

    private RemoteAuth? BuildAuth() => Auth.Kind switch
    {
        RemoteAuth.None or RemoteAuth.Oauth => Auth.Kind,
        RemoteAuth.Bearer => Token.Trim() is { Length: > 0 } token ? new RemoteAuth.Bearer(token) : null,
        RemoteAuth.Headers => Pairs(Headers) is { } headers ? new RemoteAuth.Headers(headers) : null,
        RemoteAuth.OauthClient => ClientId.Trim() is { Length: > 0 } clientId
            ? new RemoteAuth.OauthClient(clientId)
            : null,
        _ => throw new ArgumentOutOfRangeException(nameof(Auth), Auth.Kind, null)
    };

    /// <summary>Validate gates the submit; the null returns only guard the invariant.</summary>
    private Plugin? BuildConfig()
    {
        var env = Pairs(Env);
        if (env is null)
        {
            return null;
        }

        if (!IsMcp)
        {
            // Extension and provider names come from the plugin handshake,
            // not the dialog.
            var extensionArgs = ParseArgs(Args.Trim().Length == 0 ? "[]" : Args.Trim()) ?? [];
            return new Plugin(
                string.Empty,
                Transport.Local,
                _editing?.Timeout ?? CoreDefaultTimeout,
                false,
                env,
                new PluginArgs.Local(Command.Trim(), [.. extensionArgs]));
        }

        var name = _editing?.Name ?? Name.Trim();
        var disabled = _editing?.Disabled ?? false;
        if (IsRemote)
        {
            if (BuildAuth() is not { } auth)
            {
                return null;
            }

            return new Plugin(
                name,
                Transport.Http,
                (uint)Timeout,
                disabled,
                env,
                new PluginArgs.Remote(Url.Trim(), auth));
        }

        if (ParseArgs(Args.Trim()) is not { } parsed)
        {
            return null;
        }

        return new Plugin(
            name,
            Transport.Local,
            (uint)Timeout,
            disabled,
            env,
            new PluginArgs.Local(Command.Trim(), [.. parsed]));
    }

    /// <summary>True means saved and the dialog can close; failures land in
    /// SubmitError and the form stays up.</summary>
    public async Task<bool> SubmitAsync()
    {
        if (_submitting || BuildConfig() is not { } config)
        {
            return false;
        }

        _submitting = true;
        CanSubmit = false;
        SubmitError = string.Empty;
        try
        {
            if (_editing is not null)
            {
                await _client.UpdatePluginAsync(_kind, config);
            }
            else if (!IsMcp)
            {
                await (_kind == PluginType.Extension
                    ? _client.AddExtensionPluginAsync(config)
                    : _client.AddProviderPluginAsync(config));
            }
            else
            {
                await ConnectMcpAsync(config);
            }

            return true;
        }
        catch (Exception e)
        {
            if (_finalize?.IsCancellationRequested == true) return false;
            Authorizing = false;
            SubmitError = PalomaClient.Describe(e);

            return false;
        }
        finally
        {
            _submitting = false;
            Validate();
        }
    }

    /// <summary>Closing the dialog cancels whichever connect phase is in
    /// flight.</summary>
    public void Cancel() => _finalize?.Cancel();

    public void Dispose()
    {
        _finalize?.Dispose();
    }

    private async Task ConnectMcpAsync(Plugin config)
    {
        // One token spans both connect phases: closing the dialog cancels
        // whichever call is currently in flight.
        _finalize = new CancellationTokenSource();
        var session = await _client.InitMcpConnectionAsync(config);
        // A cancel that landed between the two calls must not finalize.
        _finalize.Token.ThrowIfCancellationRequested();
        if (session is not null)
        {
            Authorizing = true;
            AuthUri = Browser.Open(session.AuthUrl());
        }

        await _client.FinalizeMcpConnectionAsync(config, session, _finalize.Token);
    }
}

public sealed record AuthOption(RemoteAuth Kind, string Label);

public sealed partial class KeyValueRow(System.Action changed, Action<KeyValueRow> remove) : ObservableObject
{
    [ObservableProperty] public partial string Name { get; set; } = string.Empty;

    [ObservableProperty] public partial string Value { get; set; } = string.Empty;

    [ObservableProperty] public partial bool IsDuplicate { get; internal set; }

    public void Remove() => remove(this);

    partial void OnNameChanged(string value) => changed();
}