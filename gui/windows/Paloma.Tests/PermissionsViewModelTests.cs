using Paloma.ViewModels.Settings;
using Xunit;
using Permission = PalomaCore.Permission;

namespace Paloma.Tests;

public sealed class PermissionsViewModelTests
{
    private static Permission Permission(string prefix) => new(prefix, false, 0);

    [Fact]
    public async Task Filter_NarrowsAndReportsNoMatchesDistinctly()
    {
        var mock = new MockPalomaClient
        {
            Permissions = [Permission("git status"), Permission("dotnet build")],
        };
        var vm = new PermissionsViewModel(mock);
        await vm.LoadAsync();

        vm.Filter = "git";
        Assert.Equal("git status", Assert.Single(vm.Permissions).Prefix);

        vm.Filter = "zzz";
        Assert.Empty(vm.Permissions);
        Assert.Equal("No permissions match the search.", vm.Status);
    }

    [Fact]
    public async Task Delete_UnderFilter_PrunesTheFullSetAndKeepsTheFilter()
    {
        var mock = new MockPalomaClient
        {
            Permissions = [Permission("git status"), Permission("dotnet build")],
        };
        var vm = new PermissionsViewModel(mock);
        await vm.LoadAsync();
        vm.Filter = "git";

        await vm.DeleteAsync(vm.Permissions[0]);

        Assert.Equal("git status", Assert.Single(mock.DeletedPermissions));
        Assert.Empty(vm.Permissions);
        vm.Filter = string.Empty;
        Assert.Equal("dotnet build", Assert.Single(vm.Permissions).Prefix);
    }

    [Fact]
    public async Task Delete_WhenRpcFails_KeepsTheRowAndReportsTheError()
    {
        var mock = new MockPalomaClient
        {
            Permissions = [Permission("git status")],
            OnDeletePermission = _ =>
                throw new InvalidOperationException("storage failure"),
        };
        var vm = new PermissionsViewModel(mock);
        await vm.LoadAsync();

        await vm.DeleteAsync(vm.Permissions[0]);

        Assert.Single(vm.Permissions);
        // Failures travel on Error (the page InfoBar), not on the quiet
        // empty-state Status text.
        Assert.Contains("storage failure", vm.Error);
        Assert.Equal(string.Empty, vm.Status);
    }
}