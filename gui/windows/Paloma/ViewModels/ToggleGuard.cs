using Paloma.Client;

namespace Paloma.ViewModels;

/// <summary>Shared persist-with-revert scaffold for toggles: a failed write
/// reverts the switch and reports, and neither initialization nor the revert
/// itself echoes another write.</summary>
internal sealed class ToggleGuard
{
    private readonly string _name;
    private readonly Func<bool, Task> _persist;
    private readonly Action<bool> _revert;
    private readonly Action<string> _report;
    private bool _initializing = true;
    private bool _reverting;

    public ToggleGuard(
        string name,
        Func<bool, Task> persist,
        Action<bool> revert,
        Action<string> report)
    {
        _name = name;
        _persist = persist;
        _revert = revert;
        _report = report;
    }

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
            await _persist(value);
        }
        catch (Exception e)
        {
            // Core never stored the change; the switch must show reality.
            _reverting = true;
            _revert(!value);
            _reverting = false;
            _report($"Failed to toggle {_name}: {PalomaClient.Describe(e)}");
        }
    }
}
