using BrowserRedirect = Paloma.Provider.Runtime.V1.BrowserRedirect;
using DeviceCode = Paloma.Provider.Runtime.V1.DeviceCode;
using ManualInput = Paloma.Provider.Runtime.V1.ManualInput;

namespace Paloma.Models;

public abstract record ConnectionPhase
{
    public sealed record Loading : ConnectionPhase;

    public sealed record Challenge(DeviceCode Payload) : ConnectionPhase;

    public sealed record Manual(ManualInput Payload) : ConnectionPhase;

    public sealed record Oauth(BrowserRedirect Payload) : ConnectionPhase;

    public sealed record Success : ConnectionPhase;

    public sealed record Failed(string Message) : ConnectionPhase;
}