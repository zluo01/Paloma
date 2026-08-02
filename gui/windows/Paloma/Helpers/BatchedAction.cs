using System.Runtime.InteropServices;
using Microsoft.UI.Dispatching;

namespace Paloma.Helpers;

/// Batches a burst of triggers into one run per dispatcher pass.
/// Falls back to the constructing thread's dispatcher, then to running inline.
internal sealed class BatchedAction(Action action, Func<Action, bool>? schedule = null)
{
    private readonly Func<Action, bool>? _schedule = schedule ?? Scheduler();

    private bool _queued;

    public void Trigger()
    {
        if (_queued)
        {
            return;
        }

        _queued = true;
        if (_schedule is null || !_schedule(Run))
        {
            Run();
        }
    }

    private void Run()
    {
        _queued = false;
        action();
    }

    private static Func<Action, bool>? Scheduler()
    {
        try
        {
            return DispatcherQueue.GetForCurrentThread() is { } dispatcher
                ? run => dispatcher.TryEnqueue(DispatcherQueuePriority.Low, () => run())
                : null;
        }
        catch (COMException)
        {
            // Unit tests run without the WinUI runtime; the lookup itself throws.
            return null;
        }
    }
}
