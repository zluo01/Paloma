using System.Text.Json;
using CommunityToolkit.Mvvm.ComponentModel;
using PalomaCore;
using Paloma.Client;
using Paloma.Helpers;

namespace Paloma.ViewModels.Settings;

public sealed partial class PluginDialogViewModel : ObservableObject, IDisposable
{
    // Mirrors core's stored default (db/queries/init_table.sql: timeout
    // DEFAULT 300); the customized-settings detection breaks silently if
    // they drift.
    private const uint CoreDefaultTimeout = 300;

    private readonly IPalomaClient _client;
    private readonly IReadOnlySet<string> _takenNames;
    private readonly PluginType _kind;
    private readonly Plugin? _editing;
    private CancellationTokenSource? _finalize;
    private bool _submitting;

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

    [ObservableProperty] public partial bool RequiresAuth { get; set; }

    [ObservableProperty] public partial double Timeout { get; set; } = CoreDefaultTimeout;

    [ObservableProperty] public partial string Env { get; set; } = "{}";

    [ObservableProperty] public partial string? EnvError { get; private set; }

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
        Env = JsonSerializer.Serialize(_editing.Env);
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
                RequiresAuth = remote.RequiresAuth;
                break;
        }
    }

    partial void OnNameChanged(string value) => Validate();

    partial void OnTypeIndexChanged(int value) => Validate();

    partial void OnCommandChanged(string value) => Validate();

    partial void OnArgsChanged(string value) => Validate();

    partial void OnUrlChanged(string value) => Validate();

    partial void OnTimeoutChanged(double value) => Validate();

    partial void OnEnvChanged(string value) => Validate();

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

    private static Dictionary<string, string>? ParseEnv(string text)
    {
        if (text.Trim().Length == 0)
        {
            return [];
        }

        try
        {
            return JsonSerializer.Deserialize<Dictionary<string, string>>(text);
        }
        catch (JsonException)
        {
            return null;
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
        }

        UrlError = urlError;

        string? envError = null;
        if (ParseEnv(Env) is null)
        {
            envError = "Must be a JSON object like {\"KEY\": \"value\"}.";
            valid = false;
            // A collapsed expander must not swallow its own error.
            AdvancedExpanded = true;
        }

        EnvError = envError;

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

    /// <summary>Validate gates the submit; the null returns only guard the invariant.</summary>
    private Plugin? BuildConfig()
    {
        var env = ParseEnv(Env);
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
            return new Plugin(
                name,
                Transport.Http,
                (uint)Timeout,
                disabled,
                env,
                new PluginArgs.Remote(Url.Trim(), RequiresAuth));
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
        var session = await _client.InitMcpConnectionAsync(config, _finalize.Token);
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