using Windows.UI;
using CommunityToolkit.Mvvm.Messaging;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Paloma.Extensions;
using Paloma.Messages;
using Paloma.Models;
using Paloma.ViewModels.Overlay;
using HealthLevel = PalomaCore.HealthLevel;
using HealthStatus = PalomaCore.HealthStatus;
using Model = PalomaCore.Model;
using ProviderBackendId = PalomaCore.ProviderBackendId;

namespace Paloma.Views.Overlay.Footer;

public sealed partial class FooterView
{
    // requires to listen on mode change
    public static readonly DependencyProperty ModeProperty = DependencyProperty.Register(
        nameof(Mode), typeof(OverlayMode), typeof(FooterView), new PropertyMetadata(OverlayMode.Search));

    private static readonly SolidColorBrush InactiveHealthBrush =
        new(Color.FromArgb(0x66, 0x80, 0x80, 0x80));

    public FooterViewModel ViewModel { get; }

    public OverlayMode Mode
    {
        get => (OverlayMode)GetValue(ModeProperty);
        set => SetValue(ModeProperty, value);
    }

    public event EventHandler? SessionsRequested;

    public event EventHandler? ModelFlyoutClosed;

    public FooterView()
    {
        ViewModel = new FooterViewModel(App.Current.Client);
        InitializeComponent();
        WeakReferenceMessenger.Default.Register<OverlayShownMessage>(
            this, (_, _) => _ = ViewModel.RefreshAsync());
    }

    public static Brush HealthBrush(HealthLevel health)
    {
        return health switch
        {
            HealthLevel.Healthy => (Brush)Application.Current.Resources["SystemFillColorSuccessBrush"],
            HealthLevel.Degraded => (Brush)Application.Current.Resources["SystemFillColorCautionBrush"],
            HealthLevel.Down => (Brush)Application.Current.Resources["SystemFillColorCriticalBrush"],
            _ => InactiveHealthBrush,
        };
    }

    // Exclude following buttons from the dragging area
    internal IReadOnlyList<FrameworkElement> InteractiveControls()
    {
        return [ModelButton, SettingsButton, SessionsButton];
    }

    private void OnModelFlyoutOpening(object sender, object args)
    {
        BuildModelMenu();
    }

    private void OnModelFlyoutClosed(object sender, object args)
    {
        ModelFlyoutClosed?.Invoke(this, EventArgs.Empty);
    }

    private void BuildModelMenu()
    {
        ModelFlyout.Items.Clear();
        foreach (var connector in ViewModel.Connected)
        {
            if (connector.Connection is not { Status: { } status })
            {
                continue;
            }

            var backend = Display.Backend(connector.Id);
            if (status.Status != HealthStatus.Running)
            {
                ModelFlyout.Items.Add(DisabledItem(backend));
                continue;
            }

            var provider = new MenuFlyoutSubItem { Text = backend };
            if (status.Models.Any(model => model.Id == ViewModel.SelectedModelId))
            {
                provider.Icon = CheckIcon();
            }

            foreach (var model in status.Models)
            {
                // skip if there is no effort for the model
                if (model.SupportedReasoningEfforts.Length == 0)
                {
                    continue;
                }

                var isCurrentModel = model.Id == ViewModel.SelectedModelId;
                if (model.SupportedReasoningEfforts.Length > 1)
                {
                    var efforts = new MenuFlyoutSubItem { Text = model.Name };
                    if (isCurrentModel)
                    {
                        efforts.Icon = CheckIcon();
                    }

                    foreach (var effort in model.SupportedReasoningEfforts)
                    {
                        efforts.Items.Add(SelectItem(
                            effort,
                            isCurrentModel && effort == ViewModel.SelectedEffort,
                            connector.Id,
                            model,
                            effort));
                    }

                    provider.Items.Add(efforts);
                }
                else
                {
                    provider.Items.Add(SelectItem(
                        model.Name,
                        isCurrentModel,
                        connector.Id,
                        model,
                        model.DefaultReasoningEffort));
                }
            }

            if (provider.Items.Count == 0)
            {
                ModelFlyout.Items.Add(DisabledItem(backend));
            }
            else
            {
                ModelFlyout.Items.Add(provider);
            }
        }
    }

    private RadioMenuFlyoutItem SelectItem(
        string text,
        bool isChecked,
        ProviderBackendId backend,
        Model model,
        string effort)
    {
        var item = new RadioMenuFlyoutItem
        {
            Text = text,
            GroupName = "model-effort",
            IsChecked = isChecked,
        };
        item.Click += async (_, _) => await ViewModel.SelectModelAsync(backend, model, effort);
        return item;
    }

    private void OnSettingsClick(object sender, RoutedEventArgs args)
    {
        App.Current.ShowSettings();
    }

    private void OnSessionsClick(object sender, RoutedEventArgs args)
    {
        SessionsRequested?.Invoke(this, EventArgs.Empty);
    }

    private static FontIcon CheckIcon()
    {
        return new FontIcon { Glyph = "\uE73E" };
    }

    private static MenuFlyoutItem DisabledItem(string text)
    {
        return new MenuFlyoutItem { Text = text, IsEnabled = false };
    }
}