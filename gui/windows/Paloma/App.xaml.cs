using System.Drawing;
using Windows.Win32;
using Windows.Win32.Foundation;
using Windows.Win32.UI.WindowsAndMessaging;
using CommunityToolkit.Mvvm.Input;
using CommunityToolkit.Mvvm.Messaging;
using H.NotifyIcon;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Paloma.Client;
using Paloma.Messages;
using Paloma.Settings;
using Paloma.Views.Overlay;
using Paloma.Views.Settings;
using Serilog;
using PalomaApp = PalomaCore.PalomaApp;
using PalomaMethods = PalomaCore.PalomaMethods;

namespace Paloma;

public partial class App
{
    private SettingsWindow? _settingsWindow;
    private OverlayWindow? _overlayWindow;
    private TaskbarIcon? _tray;

    public PalomaClient Client { get; private set; } = null!;

    public AppSettings Settings { get; private set; } = null!;

    public new static App Current => (App)Application.Current;

    public App()
    {
        InitializeComponent();
        // Log unhandled UI-thread exceptions before the process dies
        UnhandledException += (_, args) =>
            Log.Fatal(args.Exception, "unhandled dispatcher exception");
    }

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        try
        {
            var local = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
            PalomaMethods.InitLogging(local);
            // Core builds on its own Tokio runtime, but the await would resume on the UI thread
            // that GetResult blocks; Task.Run keeps the wait off the UI thread so it cannot deadlock.
            var core = Task.Run(() => PalomaApp.PalomaAppAsync(local)).GetAwaiter().GetResult();
            Client = new PalomaClient(core);
        }
        catch (Exception e)
        {
            Log.Fatal(e, "core start failed");
            ShowError($"Paloma cannot start: {PalomaClient.Describe(e)}");
            Environment.Exit(1);
        }

        _overlayWindow = new OverlayWindow();
        Settings = AppSettings.Load();
        WeakReferenceMessenger.Default.Register<HotKeyPressedMessage>(
            this,
            (_, _) => _overlayWindow?.Toggle());

        _tray = BuildTray();
    }

    private TaskbarIcon BuildTray()
    {
        var open = new MenuFlyoutItem
        {
            Text = "Open Paloma",
            Command = new RelayCommand(() => _overlayWindow?.Show()),
        };
        var settings = new MenuFlyoutItem
        {
            Text = "Settings",
            Command = new RelayCommand(ShowSettings),
        };
        var quit = new MenuFlyoutItem
        {
            Text = "Quit",
            Command = new RelayCommand(Quit),
        };

        var flyout = new MenuFlyout();
        flyout.Items.Add(open);
        flyout.Items.Add(settings);
        flyout.Items.Add(new MenuFlyoutSeparator());
        flyout.Items.Add(quit);

        var tray = new TaskbarIcon
        {
            ToolTipText = "Paloma",
            Icon = new Icon(
                Path.Combine(AppContext.BaseDirectory, "Assets", "AppIcon.ico")),
            ContextFlyout = flyout,
            LeftClickCommand = new RelayCommand(ShowSettings),
        };
        tray.ForceCreate();
        return tray;
    }

    public void ShowSettings()
    {
        if (_settingsWindow is null)
        {
            _settingsWindow = new SettingsWindow();
            _settingsWindow.AppWindow.Closing += (_, closing) =>
            {
                closing.Cancel = true;
                _settingsWindow!.AppWindow.Hide();
            };
        }

        // The setting position is cached,
        // always check and span the setting window on the current user focus monitor 
        if (!_settingsWindow.AppWindow.IsVisible)
        {
            _settingsWindow.MoveToFocusedMonitor();
        }

        _settingsWindow.AppWindow.Show();
        _settingsWindow.Activate();
    }

    private void Quit()
    {
        _tray?.Dispose();
        Settings.Dispose();
        _overlayWindow?.Close();
        Client.Dispose();
        Exit();
    }

    /// Shows a native error dialog
    private static void ShowError(string message)
    {
        PInvoke.MessageBox(
            HWND.Null,
            message,
            "Paloma",
            MESSAGEBOX_STYLE.MB_OK | MESSAGEBOX_STYLE.MB_ICONERROR);
    }
}