using Connector = PalomaCore.Connector;
using ConnectorConnection = PalomaCore.ConnectorConnection;
using HealthStatus = PalomaCore.HealthStatus;
using Model = PalomaCore.Model;
using ProviderBackendId = PalomaCore.ProviderBackendId;
using Plugin = PalomaCore.Plugin;
using PluginArgs = PalomaCore.PluginArgs;
using ProviderStatus = PalomaCore.ProviderStatus;
using Transport = PalomaCore.Transport;

namespace Paloma.Tests;

/// Shared connector and model builders.
internal static class TestProtos
{
    public static readonly Model ModelA = TestModel("a", "Model A", "medium", "low", "medium");

    public static readonly Model ModelB = TestModel("b", "Model B", "high", "high", "medium");

    public static Model TestModel(
        string id,
        string name,
        string defaultEffort,
        params string[] efforts)
    {
        return new Model(id, name, defaultEffort, efforts);
    }

    public static ProviderBackendId Backend(string backend)
    {
        return new ProviderBackendId("provider", backend);
    }

    public static Connector ConnectorWith(
        string backend = "backend",
        bool preferred = false,
        string preferModel = "",
        string preferEffort = "",
        HealthStatus health = HealthStatus.Running,
        string? error = null,
        IReadOnlyList<Model>? models = null)
    {
        var status = new ProviderStatus([.. models ?? [ModelA, ModelB]], health, error);
        return new Connector(
            Backend(backend),
            "a test connector",
            null,
            new ConnectorConnection(preferred, preferModel, preferEffort, status));
    }

    public static Plugin LocalPlugin(string name, string command = "", params string[] args)
    {
        return new Plugin(name, Transport.Local, 300, false, [],
            new PluginArgs.Local(command, args));
    }

    public static Connector Unconnected(string backend = "backend")
    {
        return new Connector(Backend(backend), "a test connector", null, null);
    }
}