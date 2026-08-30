using System.Globalization;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Microsoft.Windows.AppLifecycle;
using PalomaCore;
using Serilog;
using WinRT;

namespace Paloma;

/// <summary>
/// Simplify from https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/applifecycle/applifecycle-single-instance
/// </summary>
public static class Program
{
    [STAThread]
    private static void Main()
    {
        // Initialze the internal extensions. This should always at the very beginning.
        PalomaMethods.ProcessEntry();

        ComWrappersSupport.InitializeComWrappers();

        // A second instance exits immediately; the running one stays
        // reachable through the tray and the hotkey.
        var instance = AppInstance.FindOrRegisterForKey("paloma");
        if (!instance.IsCurrent)
        {
            return;
        }

        // initialize the frontend global logger
        Log.Logger = new LoggerConfiguration()
            .WriteTo.File(
                Path.Combine(
                    Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
                    "Paloma",
                    "logs",
                    "frontend-.log"),
                formatProvider: CultureInfo.InvariantCulture,
                rollingInterval: RollingInterval.Day)
            .CreateLogger();
        // Make sure we also catch any crash errors in logs
        AppDomain.CurrentDomain.UnhandledException += (_, args) =>
            Log.Fatal(args.ExceptionObject as Exception, "unhandled exception");
        TaskScheduler.UnobservedTaskException += (_, args) =>
            Log.Error(args.Exception, "unobserved task exception");
        try
        {
            Application.Start(p =>
            {
                var context = new DispatcherQueueSynchronizationContext(
                    DispatcherQueue.GetForCurrentThread());
                SynchronizationContext.SetSynchronizationContext(context);
                _ = new App();
            });
        }
        finally
        {
            Log.CloseAndFlush();
        }
    }
}