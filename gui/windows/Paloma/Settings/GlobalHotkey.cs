using System.ComponentModel;
using System.Runtime.InteropServices;
using Windows.Win32;
using Windows.Win32.Foundation;
using Windows.Win32.UI.Shell;
using CommunityToolkit.Mvvm.Messaging;
using Paloma.Messages;
using Paloma.Models;

namespace Paloma.Settings;

internal sealed partial class GlobalHotkey : IDisposable
{
    private const int Id = 1;

    private readonly HWND _hwnd;
    private readonly SUBCLASSPROC _subclassProc;
    public KeyBinding Binding { get; private set; }

    public GlobalHotkey(KeyBinding binding)
    {
        // Create a message-only windows for key event listening
        unsafe
        {
            _hwnd = PInvoke.CreateWindowEx(
                0,
                "STATIC",
                null,
                0,
                0,
                0,
                0,
                0,
                HWND.HWND_MESSAGE,
                null,
                null,
                null);
        }

        if (_hwnd == HWND.Null)
        {
            throw new Win32Exception();
        }

        // Windows requires app to register the key binding every time on startup and will unregister automatically on close.
        // register on initialization to check if the saved key binding is still available.
        if (!PInvoke.RegisterHotKey(_hwnd, Id, binding.Modifiers, (uint)binding.VirtualKey))
        {
            var error = Marshal.GetLastPInvokeError();
            PInvoke.DestroyWindow(_hwnd);
            throw new Win32Exception(error);
        }

        // on Windows, there is no way to update the original procedure directly,
        // hence we need to swap/delegate our own procedure handler for listening key binding
        // and fallback to the original procedure if not a key binding msg.
        _subclassProc = (window, message, wparam, lparam, _, _) =>
        {
            if (message != PInvoke.WM_HOTKEY || wparam.Value != Id)
            {
                return PInvoke.DefSubclassProc(window, message, wparam, lparam);
            }

            WeakReferenceMessenger.Default.Send(new HotKeyPressedMessage());
            return new LRESULT(0);
        };
        if (!PInvoke.SetWindowSubclass(_hwnd, _subclassProc, Id, 0))
        {
            var error = Marshal.GetLastPInvokeError();
            PInvoke.UnregisterHotKey(_hwnd, Id);
            PInvoke.DestroyWindow(_hwnd);
            throw new Win32Exception(error);
        }

        Binding = binding;
    }

    public bool Update(KeyBinding binding)
    {
        if (binding == Binding)
        {
            return true;
        }

        // Windows save hotkey in a list, so need to delete the original first, then set it
        PInvoke.UnregisterHotKey(_hwnd, Id);
        if (!PInvoke.RegisterHotKey(_hwnd, Id, binding.Modifiers, (uint)binding.VirtualKey))
        {
            PInvoke.RegisterHotKey(_hwnd, Id, Binding.Modifiers, (uint)Binding.VirtualKey);
            return false;
        }

        Binding = binding;
        return true;
    }

    public void Dispose()
    {
        PInvoke.RemoveWindowSubclass(_hwnd, _subclassProc, Id);
        PInvoke.UnregisterHotKey(_hwnd, Id);
        PInvoke.DestroyWindow(_hwnd);
    }
}