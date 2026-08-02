using Paloma.Helpers;
using Xunit;

namespace Paloma.Tests;

public class BatchedActionTests
{
    [Fact]
    public void Trigger_BurstWhileScheduled_RunsOnce()
    {
        var runs = 0;
        Action? pending = null;
        var batched = new BatchedAction(() => runs++, run =>
        {
            pending = run;
            return true;
        });

        batched.Trigger();
        batched.Trigger();
        batched.Trigger();

        Assert.Equal(0, runs);
        pending!();
        Assert.Equal(1, runs);
    }

    [Fact]
    public void Trigger_AfterTheRun_SchedulesAgain()
    {
        var runs = 0;
        Action? pending = null;
        var batched = new BatchedAction(() => runs++, run =>
        {
            pending = run;
            return true;
        });

        batched.Trigger();
        pending!();
        batched.Trigger();
        pending!();

        Assert.Equal(2, runs);
    }

    [Fact]
    public void Trigger_FromInsideTheRun_SchedulesTheNextRun()
    {
        var runs = 0;
        Action? pending = null;
        BatchedAction batched = null!;
        batched = new BatchedAction(
            () =>
            {
                if (++runs == 1)
                {
                    batched.Trigger();
                }
            },
            run =>
            {
                pending = run;
                return true;
            });

        batched.Trigger();
        pending!();
        Assert.Equal(1, runs);
        pending!();
        Assert.Equal(2, runs);
    }

    [Fact]
    public void Trigger_SchedulerDeclines_RunsInline()
    {
        var runs = 0;
        var batched = new BatchedAction(() => runs++, _ => false);

        batched.Trigger();

        Assert.Equal(1, runs);
    }

    [Fact]
    public void Trigger_WithoutADispatcher_RunsInline()
    {
        var runs = 0;
        var batched = new BatchedAction(() => runs++);

        batched.Trigger();
        batched.Trigger();

        Assert.Equal(2, runs);
    }
}
