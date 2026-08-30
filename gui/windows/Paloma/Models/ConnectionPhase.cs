using BrowserRedirect = PalomaCore.ConnectionPayload.BrowserRedirect;
using DeviceCode = PalomaCore.ConnectionPayload.DeviceCode;
using ManualInput = PalomaCore.ConnectionPayload.ManualInput;

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