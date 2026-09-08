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

            var backendMenu = new MenuFlyoutSubItem { Text = backend };
            if (status.Models.Any(model => model.Id == ViewModel.SelectedModelId))
            {
                backendMenu.Icon = CheckIcon();
            }

            // skip if there is no effort for the model
            var models = status.Models
                .Where(model => model.SupportedReasoningEfforts.Length > 0);
            var groups = ModelProviders(backend, models);
            // display models directly on single model provider, else show the
            // model providers menu first
            if (groups.Count == 1)
            {
                AddModelItems(backendMenu, connector.Id, groups[0].Models);
            }
            else
            {
                AddModelProviderItems(backendMenu, connector.Id, groups);
            }

            if (backendMenu.Items.Count == 0)
            {
                ModelFlyout.Items.Add(DisabledItem(backend));
            }
            else
            {
                ModelFlyout.Items.Add(backendMenu);
            }
        }
    }

    private void AddModelProviderItems(
        MenuFlyoutSubItem menu,
        ProviderBackendId backend,
        List<(string Name, List<Model> Models)> groups)
    {
        foreach (var (name, models) in groups)
        {
            var modelProvider = new MenuFlyoutSubItem { Text = name };
            if (models.Any(model => model.Id == ViewModel.SelectedModelId))
            {
                modelProvider.Icon = CheckIcon();
            }

            AddModelItems(modelProvider, backend, models);
            menu.Items.Add(modelProvider);
        }
    }

    private void AddModelItems(MenuFlyoutSubItem menu, ProviderBackendId backend, List<Model> models)
    {
        foreach (var model in models)
        {
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
                        backend,
                        model,
                        effort));
                }

                menu.Items.Add(efforts);
            }
            else
            {
                menu.Items.Add(SelectItem(
                    model.Name,
                    isCurrentModel,
                    backend,
                    model,
                    model.DefaultReasoningEffort));
            }
        }
    }

    /// <summary>Group models by their model providers, sorted by provider name.</summary>
    private static List<(string Name, List<Model> Models)> ModelProviders(
        string backend,
        IEnumerable<Model> models)
    {
        return
        [
            .. models
                .GroupBy(model => model.Provider.Length == 0 ? backend : model.Provider)
                .Select(group => (Name: group.Key, Models: group.ToList()))
                .OrderBy(pair => pair.Name, StringComparer.Ordinal),
        ];
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