using CommunityToolkit.Mvvm.ComponentModel;
using Paloma.Client;
using Paloma.Extensions;
using Paloma.Helpers;
using Paloma.Models;
using Connector = Paloma.Binding.V1.Connector;
using ProviderAuthMethod = Paloma.Provider.Runtime.V1.ProviderAuthMethod;
using ProviderBackendId = Paloma.Binding.V1.ProviderBackendId;

namespace Paloma.ViewModels.Settings;

public sealed partial class ConnectViewModel(IPalomaClient client, Connector connector) : ObservableObject
{
    private readonly ProviderBackendId _id = connector.Id;

    public string Title { get; } = $"Connect {Display.Backend(connector.Id)}";

    public string Description { get; } = connector.Description;

    [ObservableProperty] public partial ConnectionPhase Phase { get; private set; } = new ConnectionPhase.Loading();

    partial void OnPhaseChanged(ConnectionPhase value) => OnPropertyChanged(string.Empty);

    [ObservableProperty] public partial string Input { get; set; } = string.Empty;

    partial void OnInputChanged(string value) => OnPropertyChanged(nameof(HasInput));

    public bool IsLoading => Phase is ConnectionPhase.Loading;

    public bool IsChallenge => Phase is ConnectionPhase.Challenge;

    public bool IsManual => Phase is ConnectionPhase.Manual;

    public bool IsOauth => Phase is ConnectionPhase.Oauth;

    public bool IsSuccess => Phase is ConnectionPhase.Success;

    public bool IsFailed => Phase is ConnectionPhase.Failed;

    public bool NeedsInput => IsManual || IsOauth;

    public bool HasInput => !string.IsNullOrWhiteSpace(Input);

    public string UserCode =>
        Phase is ConnectionPhase.Challenge challenge ? challenge.Payload.UserCode : string.Empty;

    public string ErrorMessage =>
        Phase is ConnectionPhase.Failed failed ? failed.Message : string.Empty;

    public Uri? VerificationUri =>
        Phase is ConnectionPhase.Challenge challenge
            ? ToUri(challenge.Payload.VerificationUrl)
            : null;

    public Uri? InstructionsUri =>
        Phase is ConnectionPhase.Manual { Payload: { HasInstructionsUrl: true } payload }
            ? ToUri(payload.InstructionsUrl)
            : null;

    public Uri? AuthorizationUri =>
        Phase is ConnectionPhase.Oauth oauth ? ToUri(oauth.Payload.AuthorizationUrl) : null;

    public bool HasInstructions => InstructionsUri is not null;

    /// <summary>An empty label hides the button; only typed-input phases
    /// have anything for Connect to submit.</summary>
    public string PrimaryLabel => NeedsInput ? "Connect" : string.Empty;

    public string CloseLabel => IsSuccess || IsFailed ? "Close" : "Cancel";

    private static Uri? ToUri(string? url) =>
        url is not null && Uri.TryCreate(url, UriKind.Absolute, out var uri) ? uri : null;

    public async Task StartAsync()
    {
        Phase = await GuardAsync(() => client.InitConnectionAsync(_id));
        switch (Phase)
        {
            case ConnectionPhase.Challenge challenge:
                Browser.Open(challenge.Payload.VerificationUrl);
                Phase = await FinalizeAsync(
                    ProviderAuthMethod.DeviceCode, challenge.Payload.TransactionPayload);
                break;
            case ConnectionPhase.Oauth oauth:
                Browser.Open(oauth.Payload.AuthorizationUrl);
                break;
        }
    }

    public async Task SubmitAsync()
    {
        var method = Phase switch
        {
            ConnectionPhase.Manual => ProviderAuthMethod.ApiKey,
            ConnectionPhase.Oauth => ProviderAuthMethod.BrowserOauth,
            _ => (ProviderAuthMethod?)null,
        };
        if (method is null || string.IsNullOrWhiteSpace(Input))
        {
            return;
        }

        var payload = Input.Trim();
        Phase = new ConnectionPhase.Loading();
        Phase = await FinalizeAsync(method.Value, payload);
    }

    public async Task CancelAsync()
    {
        if (Phase is ConnectionPhase.Success or ConnectionPhase.Failed)
        {
            return;
        }

        try
        {
            await client.CancelConnectionAsync(_id);
        }
        catch
        {
            // Cancelling a connection that never started has nothing to undo.
        }
    }

    private Task<ConnectionPhase> FinalizeAsync(ProviderAuthMethod method, string payload) =>
        GuardAsync(async () =>
        {
            await client.FinalizeConnectionAsync(_id, method, payload);
            return new ConnectionPhase.Success();
        });

    private static async Task<ConnectionPhase> GuardAsync(Func<Task<ConnectionPhase>> operation)
    {
        try
        {
            return await operation();
        }
        catch (Exception e)
        {
            return new ConnectionPhase.Failed(PalomaClient.Describe(e));
        }
    }
}