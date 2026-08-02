using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Messaging;
using Paloma.Messages;

namespace Paloma.ViewModels.Overlay;

public sealed partial class OverlayViewModel : ObservableObject, IDisposable
{
    private static readonly TimeSpan ErrorBannerDuration = TimeSpan.FromSeconds(6);

    private readonly TimeSpan _bannerDuration;
    private CancellationTokenSource? _errorCts;

    [ObservableProperty] public partial string ErrorMessage { get; private set; } = string.Empty;

    public OverlayViewModel(IMessenger? messenger = null, TimeSpan? bannerDuration = null)
    {
        _bannerDuration = bannerDuration ?? ErrorBannerDuration;
        (messenger ?? WeakReferenceMessenger.Default).Register<ErrorReportedMessage>(
            this, (banner, message) => ((OverlayViewModel)banner).ReportError(message.Message));
    }

    public void Dispose()
    {
        _errorCts?.Dispose();
    }

    private async void ReportError(string message)
    {
        _errorCts?.Cancel();
        var cts = _errorCts = new CancellationTokenSource();
        ErrorMessage = message;
        try
        {
            await Task.Delay(_bannerDuration, cts.Token);
        }
        catch (TaskCanceledException)
        {
            return;
        }

        // A newer banner can land after the delay expires but before this
        // line runs. Only the current banner may clear the message.
        if (_errorCts == cts)
        {
            ErrorMessage = string.Empty;
        }
    }
}