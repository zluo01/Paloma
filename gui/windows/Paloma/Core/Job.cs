using System.ComponentModel;
using System.Diagnostics;
using Microsoft.Win32.SafeHandles;
using Windows.Win32;
using Windows.Win32.Foundation;
using Windows.Win32.System.JobObjects;

namespace Paloma.Core;

/// <summary>
/// Job object with KILL_ON_JOB_CLOSE: the kernel terminates every assigned
/// process when the last handle closes, so paloma-core dies with this GUI
/// even on a crash — Dispose only has to close the handle.
/// </summary>
internal sealed partial class Job : IDisposable
{
    private readonly SafeFileHandle _handle;

    public Job()
    {
        _handle = PInvoke.CreateJobObject();
        if (_handle.IsInvalid)
        {
            throw new Win32Exception();
        }

        var info = new JOBOBJECT_EXTENDED_LIMIT_INFORMATION();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT.JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        unsafe
        {
            // no SafeHandle overload is generated for pointer-taking
            // signatures; _handle stays alive through this instance
            if (!PInvoke.SetInformationJobObject(
                    new HANDLE(_handle.DangerousGetHandle()),
                    JOBOBJECTINFOCLASS.JobObjectExtendedLimitInformation,
                    &info,
                    (uint)sizeof(JOBOBJECT_EXTENDED_LIMIT_INFORMATION)))
            {
                throw new Win32Exception();
            }
        }
    }

    public void Assign(Process process)
    {
        if (!PInvoke.AssignProcessToJobObject(_handle, process.SafeHandle))
        {
            throw new Win32Exception();
        }
    }

    public void Dispose() => _handle.Dispose();
}
