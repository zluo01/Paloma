namespace Paloma.Messages;

/// <summary>Broadcast by any view model whose call failed; the overlay's
/// error banner shows the message.</summary>
public sealed record ErrorReportedMessage(string Message);