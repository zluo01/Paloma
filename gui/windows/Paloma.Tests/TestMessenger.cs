using CommunityToolkit.Mvvm.Messaging;
using Paloma.Messages;

namespace Paloma.Tests;

internal static class TestMessenger
{
    /// A private bus per test: the global one is shared across parallel
    /// test classes. Reported errors collect into the returned list.
    public static (IMessenger Messenger, List<string> Errors) WithErrorSink()
    {
        var messenger = new WeakReferenceMessenger();
        var errors = new List<string>();
        messenger.Register<List<string>, ErrorReportedMessage>(
            errors, (list, message) => list.Add(message.Message));
        return (messenger, errors);
    }
}
