using Connector = Paloma.Binding.V1.Connector;
using ConnectorConnection = Paloma.Binding.V1.ConnectorConnection;
using HealthStatus = Paloma.Binding.V1.HealthStatus;
using Model = Paloma.Provider.Runtime.V1.Model;
using ProviderBackendId = Paloma.Binding.V1.ProviderBackendId;
using ProviderStatus = Paloma.Binding.V1.ProviderStatus;

namespace Paloma.Tests;

/// Shared connector and model proto builders.
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
        var model = new Model { Id = id, Name = name, DefaultReasoningEffort = defaultEffort };
        model.SupportedReasoningEfforts.AddRange(efforts);
        return model;
    }

    public static ProviderBackendId Backend(string backend)
    {
        return new ProviderBackendId { ProviderId = "provider", BackendId = backend };
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
        var status = new ProviderStatus
        {
            Status = health,
            Models = { models ?? [ModelA, ModelB] },
        };
        if (error is not null)
        {
            status.Error = error;
        }

        return new Connector
        {
            Id = Backend(backend),
            Description = "a test connector",
            Connection = new ConnectorConnection
            {
                Preferred = preferred,
                PreferModel = preferModel,
                PreferEffort = preferEffort,
                Status = status,
            },
        };
    }

    public static Connector Unconnected(string backend = "backend")
    {
        return new Connector { Id = Backend(backend), Description = "a test connector" };
    }
}
