using Paloma.Client;

namespace Paloma.ViewModels;

/// <summary>Shared persist-with-revert scaffold for toggles: a failed write
/// reverts the switch and reports, and neither initialization nor the revert
/// itself echoes another write.</summary>
internal sealed class ToggleGuard(
    string name,
    Func<bool, Task> persist,
    Action<bool> revert,
    Action<string> report)
{
    private bool _initializing = true;
    private bool _reverting;

    public void Ready() => _initializing = false;

    public void Changed(bool value)
    {
        if (_initializing || _reverting)
        {
            return;
        }

        _ = PersistAsync(value);
    }

    private async Task PersistAsync(bool value)
    {
        try
        {
            await persist(value);
        }
        catch (Exception e)
        {
            // Core never stored the change; the switch must show reality.
            _reverting = true;
            revert(!value);
            _reverting = false;
            report($"Failed to toggle {name}: {PalomaClient.Describe(e)}");
        }
    }
}