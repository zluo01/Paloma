using System.Diagnostics;
using Serilog;

namespace Paloma.Core;

/// <summary>
/// Starts and owns the paloma-core.exe child process. Core serves gRPC on a
/// pipe named after this process id, and is assigned to a <see cref="Job"/>
/// so the OS kills it automatically when this app exits, even on a crash.
/// </summary>
public sealed partial class CoreProcess : IDisposable
{
    private readonly Job _job;
    private readonly Process _process;

    public string PipeName { get; }

    private CoreProcess(Job job, Process process, string pipeName)
    {
        _job = job;
        _process = process;
        PipeName = pipeName;
    }

    /// <summary>
    /// Starts core and invokes <paramref name="onExited"/> only when it
    /// stops on its own; quitting the app or disposing never fires it.
    /// The handler must be attached before exit events are enabled: a
    /// child that already died raises the event the moment they turn on.
    /// </summary>
    public static CoreProcess Start(Action onExited)
    {
        var corePath = GetCorePath();
        var pipeName = $"paloma-{Environment.ProcessId}";

        var job = new Job();
        var process = new Process
        {
            StartInfo = new ProcessStartInfo
            {
                FileName = corePath,
                Arguments = $"--pipe {pipeName}",
                UseShellExecute = false,
                CreateNoWindow = true,
                RedirectStandardError = true,
            },
        };
        try
        {
            // Accepted gap: an external kill of this app between Start and
            // Assign orphans core until reboot. Closing it needs a raw
            // CreateProcess with CREATE_SUSPENDED; Process cannot spawn
            // suspended, and the window is sub-millisecond.
            process.Start();
            job.Assign(process);
        }
        catch
        {
            try
            {
                process.Kill();
            }
            catch
            {
                // never started, or already gone
            }

            process.Dispose();
            job.Dispose();
            throw;
        }

        process.Exited += (_, _) => onExited();
        process.EnableRaisingEvents = true;

        process.ErrorDataReceived += (_, args) =>
        {
            if (args.Data is not { } line) return;
            Debug.WriteLine($"[paloma-core] {line}");
            Log.Information("[core] {Line:l}", line);
        };
        process.BeginErrorReadLine();
        return new CoreProcess(job, process, pipeName);
    }

    private static string GetCorePath()
    {
        var exe = Path.Combine(AppContext.BaseDirectory, "paloma-core.exe");
        return !File.Exists(exe) ? throw new FileNotFoundException("Failed to locate paloma-core.exe.", exe) : exe;
    }

    public void Dispose()
    {
        // do not signal error exit path on normal dispose.
        _process.EnableRaisingEvents = false;
        _process.Dispose();
        _job.Dispose();
    }
}