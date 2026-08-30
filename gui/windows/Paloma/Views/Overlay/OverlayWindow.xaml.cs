using System.Collections.Specialized;
using System.ComponentModel;
using System.Drawing;
using System.Runtime.InteropServices;
using Windows.Graphics;
using Windows.Win32;
using Windows.Win32.Foundation;
using Windows.Win32.Graphics.Dwm;
using Windows.Win32.UI.Input.KeyboardAndMouse;
using Windows.Win32.UI.Shell;
using Windows.Win32.UI.WindowsAndMessaging;
using CommunityToolkit.Mvvm.Messaging;
using Microsoft.UI.Input;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Paloma.Helpers;
using Paloma.Messages;
using Paloma.Models;
using Paloma.ViewModels.Overlay;
using Serilog;
using WinRT.Interop;
using Size = Windows.Foundation.Size;

namespace Paloma.Views.Overlay;

public sealed partial class OverlayWindow
{
    private const int OverlayWidth = 680;
    private const int MaxOverlayHeight = 540;

    private readonly BatchedAction _resize;

    // The window starts at a meaningless default position,
    // so the first summon always moves it into place.
    private bool _hasBeenPlaced;

    // Keep the subclass procedure so it does not get GC
    private readonly SUBCLASSPROC _activationProc;
    private readonly HOOKPROC _clickAwayProc;

    // Installed only while the overlay is visible.
    private UnhookWindowsHookExSafeHandle? _clickAwayHook;

    public OverlayWindow()
    {
        InitializeComponent();
        _resize = new BatchedAction(ResizeWindow);

        var presenter = OverlappedPresenter.Create();
        presenter.SetBorderAndTitleBar(false, false);
        presenter.IsAlwaysOnTop = true;
        presenter.IsResizable = false;
        presenter.IsMinimizable = false;
        presenter.IsMaximizable = false;
        AppWindow.SetPresenter(presenter);
        AppWindow.IsShownInSwitchers = false;
        var hwnd = WindowNative.GetWindowHandle(this);
        ConvertToPopup(hwnd);
        ApplyFrameStyle(hwnd);

        // procedure to listen windows activate message
        _activationProc = OnActivateMessage;
        _ = PInvoke.SetWindowSubclass(new HWND(hwnd), _activationProc, 1, 0);
        // click to hide callback procedure
        _clickAwayProc = OnGlobalClick;

        // resize for the search result content change
        Overlay.Search.Groups.CollectionChanged += OnGroupsChanged;
        // resize for the error banner show or hide
        Overlay.ViewModel.PropertyChanged += OnViewModelPropertyChanged;
    }

    public void Toggle()
    {
        try
        {
            if (AppWindow.IsVisible)
            {
                Hide();
            }
            else
            {
                Show();
            }
        }
        catch (Exception e)
        {
            Log.Error(e, "toggle failed");
        }
    }

    public void Show()
    {
        WindowPlacement.SizeAndPlaceOnCursorMonitor(
            AppWindow, OverlayWidth, ContentHeight(), keepIfOnMonitor: _hasBeenPlaced);
        _hasBeenPlaced = true;

        Activate();
        var hwnd = WindowNative.GetWindowHandle(this);

        // set some styles
        ApplyFrameStyle(hwnd);
        WindowFrame.MatchFrameToTheme(
            hwnd,
            Overlay.ActualTheme == ElementTheme.Dark);

        // make sure we keep the launcher on top
        ForceForeground(hwnd);
        Overlay.FocusInput();

        // click to hide procedure is a global event,
        // hence only add the click to hide hook procedure on showing
        // such that it does not affect other running apps during hiding.
        _clickAwayHook ??= PInvoke.SetWindowsHookEx(
            WINDOWS_HOOK_ID.WH_MOUSE_LL, _clickAwayProc, null, 0);

        QueueResize();
    }

    private void Hide()
    {
        // explicitly drop the click on hide hook
        // so it does not affect other apps during hidden
        _clickAwayHook?.Dispose();
        _clickAwayHook = null;
        AppWindow.Hide();
    }

    private void OnVisibilityChanged(object sender, WindowVisibilityChangedEventArgs args)
    {
        if (args.Visible)
        {
            WeakReferenceMessenger.Default.Send(new OverlayShownMessage());
        }
        else
        {
            WeakReferenceMessenger.Default.Send(new OverlayHiddenMessage());
        }
    }

    // A search response updates the results in several steps.
    // Collapse them into a single resize instead of one per step.
    private void QueueResize()
    {
        _resize.Trigger();
    }

    /// resize the app window to make sure the window boundary match the inner content size
    private void ResizeWindow()
    {
        var scale = Overlay.XamlRoot.RasterizationScale;
        AppWindow.Resize(new SizeInt32(
            (int)(OverlayWidth * scale),
            (int)(ContentHeight() * scale)));
    }

    // compute the current display content height
    // chat and session mode has fix height
    // search mode height change along with the search result size
    private double ContentHeight()
    {
        if (Overlay.Mode != OverlayMode.Search)
        {
            return MaxOverlayHeight;
        }

        Overlay.Measure(new Size(OverlayWidth, double.PositiveInfinity));
        return Math.Min(Overlay.DesiredSize.Height, MaxOverlayHeight);
    }

    // recompute the required available height when search result changes
    private void OnGroupsChanged(object? sender, NotifyCollectionChangedEventArgs args)
    {
        QueueResize();
    }

    // The system move loop handles the dragging; interactive controls
    // are carved back out of the draggable area.
    private void OnDragRegionsChanged(RectInt32[] caption, RectInt32[] passthrough)
    {
        var source = InputNonClientPointerSource.GetForWindowId(AppWindow.Id);
        source.SetRegionRects(NonClientRegionKind.Caption, caption);
        source.SetRegionRects(NonClientRegionKind.Passthrough, passthrough);
    }

    // recompute the required window height when error banner shows or hides
    private void OnViewModelPropertyChanged(object? sender, PropertyChangedEventArgs args)
    {
        if (args.PropertyName == nameof(OverlayViewModel.ErrorMessage))
        {
            QueueResize();
        }
    }

    private LRESULT OnActivateMessage(
        HWND hwnd, uint msg, WPARAM wparam, LPARAM lparam, nuint id, nuint refData)
    {
        // not windows activate message, skip
        if (msg != PInvoke.WM_ACTIVATE) return PInvoke.DefSubclassProc(hwnd, msg, wparam, lparam);

        var state = (uint)(wparam.Value & 0xFFFF);
        if (state != PInvoke.WA_INACTIVE)
        {
            Overlay.FocusInput();
        }
        else if (AppWindow.IsVisible)
        {
            Hide();
        }

        return PInvoke.DefSubclassProc(hwnd, msg, wparam, lparam);
    }

    // a global click event to handle click to hide launcher.
    // should only bound when launcher is shown and closed on hide
    // to prevent affecting other running programs
    private LRESULT OnGlobalClick(int code, WPARAM wparam, LPARAM lparam)
    {
        if (code < 0
            || (uint)wparam.Value
            is not (PInvoke.WM_LBUTTONDOWN or PInvoke.WM_RBUTTONDOWN or PInvoke.WM_MBUTTONDOWN))
            return PInvoke.CallNextHookEx(HHOOK.Null, code, wparam, lparam);
        var point = Marshal.PtrToStructure<MSLLHOOKSTRUCT>(lparam).pt;
        if (!InsideAppWindows(point) && AppWindow.IsVisible)
        {
            DispatcherQueue.TryEnqueue(Hide);
        }

        return PInvoke.CallNextHookEx(HHOOK.Null, code, wparam, lparam);
    }

    /// A flyout is a separate window that can overhang the frame.
    /// A click counts as inside when it lands on any visible app window.
    private static bool InsideAppWindows(Point point)
    {
        var inside = false;
        WNDENUMPROC test = (candidate, _) =>
        {
            if (!PInvoke.IsWindowVisible(candidate))
            {
                return true;
            }

            PInvoke.GetWindowRect(candidate, out var rect);
            if (point.X < rect.left || point.X >= rect.right
                                    || point.Y < rect.top || point.Y >= rect.bottom) return true;
            inside = true;
            return false;

        };
        _ = PInvoke.EnumThreadWindows(PInvoke.GetCurrentThreadId(), test, new LPARAM(0));
        return inside;
    }

    /// Rounds the window corners and keeps the frame composited.
    /// An uncomposited frame paints a light hairline across the top edge.
    private static void ApplyFrameStyle(nint hwnd)
    {
        WindowFrame.SetAttribute(
            hwnd,
            DWMWINDOWATTRIBUTE.DWMWA_WINDOW_CORNER_PREFERENCE,
            (int)DWM_WINDOW_CORNER_PREFERENCE.DWMWCP_ROUND);
        WindowFrame.SetAttribute(
            hwnd,
            DWMWINDOWATTRIBUTE.DWMWA_NCRENDERING_POLICY,
            (int)DWMNCRENDERINGPOLICY.DWMNCRP_ENABLED);
    }

    /// Turns the window into a popup window. The normal window
    /// type paints a caption line at some display scales.
    private static void ConvertToPopup(nint hwnd)
    {
        var handle = new HWND(hwnd);
        var style = (WINDOW_STYLE)(nuint)PInvoke.GetWindowLongPtr(
            handle,
            WINDOW_LONG_PTR_INDEX.GWL_STYLE);
        style &= ~(WINDOW_STYLE.WS_CAPTION
                   | WINDOW_STYLE.WS_THICKFRAME
                   | WINDOW_STYLE.WS_SYSMENU
                   | WINDOW_STYLE.WS_MINIMIZEBOX
                   | WINDOW_STYLE.WS_MAXIMIZEBOX);
        style |= WINDOW_STYLE.WS_POPUP;
        _ = PInvoke.SetWindowLongPtr(handle, WINDOW_LONG_PTR_INDEX.GWL_STYLE, (nint)style);
        // Apply the style change without moving, sizing, reordering,
        // or activating the window.
        _ = PInvoke.SetWindowPos(
            handle,
            HWND.Null,
            0,
            0,
            0,
            0,
            SET_WINDOW_POS_FLAGS.SWP_FRAMECHANGED
            | SET_WINDOW_POS_FLAGS.SWP_NOMOVE
            | SET_WINDOW_POS_FLAGS.SWP_NOSIZE
            | SET_WINDOW_POS_FLAGS.SWP_NOZORDER
            | SET_WINDOW_POS_FLAGS.SWP_NOACTIVATE);
    }

    /// Takes the foreground for the window. Windows only allows
    /// this for a process with recent input, so an empty input is injected
    /// first.
    private static void ForceForeground(nint hwnd)
    {
        var handle = new HWND(hwnd);
        Span<INPUT> credit = [default];
        _ = PInvoke.SendInput(credit, Marshal.SizeOf<INPUT>());
        _ = PInvoke.SetForegroundWindow(handle);
        if (PInvoke.GetForegroundWindow() == handle)
        {
            return;
        }

        var foreground = PInvoke.GetForegroundWindow();
        var current = PInvoke.GetCurrentThreadId();
        uint target;
        unsafe
        {
            target = foreground == HWND.Null
                ? current
                : PInvoke.GetWindowThreadProcessId(foreground, null);
        }

        if (target != current)
        {
            PInvoke.AttachThreadInput(target, current, true);
            PInvoke.SetForegroundWindow(handle);
            PInvoke.BringWindowToTop(handle);
            PInvoke.AttachThreadInput(target, current, false);
        }
        else
        {
            PInvoke.SetForegroundWindow(handle);
        }
    }
}