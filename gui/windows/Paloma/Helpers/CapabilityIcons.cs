using System.Runtime.InteropServices;
using System.Runtime.InteropServices.Marshalling;
using System.Runtime.InteropServices.WindowsRuntime;
using System.Security.Cryptography;
using Windows.Graphics.Imaging;
using Windows.Win32;
using Windows.Win32.Foundation;
using Windows.Win32.Graphics.Gdi;
using BitFaster.Caching;
using BitFaster.Caching.Lru;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using CapabilityIcon = Paloma.Extension.V1.CapabilityIcon;

namespace Paloma.Helpers;

/// <summary>
/// Turns a CapabilityIcon into an icon element, or null when it cannot render.
/// </summary>
public static partial class CapabilityIcons
{
    private const int RenderSize = 64;
    private const int CacheCapacity = 512;

    // Atomic so a raced miss still runs the render factory exactly once.
    private static readonly ICache<string, Task<ImageSource?>> Cache =
        new ConcurrentLruBuilder<string, Task<ImageSource?>>()
            .WithCapacity(CacheCapacity)
            .WithAtomicGetOrAdd()
            .Build();

    public static bool CanLoad(CapabilityIcon icon)
    {
        return icon.IconCase switch
        {
            CapabilityIcon.IconOneofCase.Embedded => !icon.Embedded.IsEmpty,
            CapabilityIcon.IconOneofCase.Path => icon.Path.Length > 0,
            CapabilityIcon.IconOneofCase.Name => IsGlyph(icon.Name),
            _ => false,
        };
    }

    public static async Task<IconElement?> LoadAsync(CapabilityIcon icon)
    {
        if (icon.IconCase == CapabilityIcon.IconOneofCase.Name)
        {
            return IsGlyph(icon.Name) ? new FontIcon { Glyph = icon.Name } : null;
        }

        var source = icon.IconCase switch
        {
            CapabilityIcon.IconOneofCase.Embedded => DecodeEmbedded(icon.Embedded.ToByteArray()),
            CapabilityIcon.IconOneofCase.Path when icon.Path.Length > 0 =>
                await LoadPathAsync(icon.Path),
            _ => null,
        };
        return source is null ? null : new ImageIcon { Source = source };
    }

    public static ImageSource? DecodeEmbedded(byte[] bytes)
    {
        if (bytes.Length == 0)
        {
            return null;
        }

        var key = Convert.ToHexString(SHA256.HashData(bytes));
        return Cache.GetOrAdd(key, _ => Task.FromResult(Decode(bytes))).Result;
    }

    internal static bool IsSvg(byte[] bytes)
    {
        var start = 0;
        if (bytes is [0xEF, 0xBB, 0xBF, ..])
        {
            start = 3;
        }

        while (start < bytes.Length
            && bytes[start] is (byte)' ' or (byte)'\t' or (byte)'\r' or (byte)'\n')
        {
            start++;
        }

        return start < bytes.Length && bytes[start] == (byte)'<';
    }

    public static ImageSource? ImageFromPath(string path)
    {
        if (!Uri.TryCreate(path, UriKind.Absolute, out var uri))
        {
            return null;
        }

        return path.EndsWith(".svg", StringComparison.OrdinalIgnoreCase)
            ? new SvgImageSource { UriSource = uri }
            : new BitmapImage { UriSource = uri };
    }

    internal static bool IsGlyph(string name)
    {
         return name is [>= '\uE000' and <= '\uF8FF'];
    }
    
    internal static Task<ImageSource?> LoadPathAsync(string path)
    {
        return Cache.GetOrAdd(path, static requested =>
        {
            var task = RenderAsync(requested);
            EvictFailure(requested, task);
            return task;
        });
    }

    private static ImageSource? Decode(byte[] bytes)
    {
        var stream = new MemoryStream(bytes).AsRandomAccessStream();
        if (IsSvg(bytes))
        {
            var svg = new SvgImageSource();
            _ = svg.SetSourceAsync(stream);
            return svg;
        }

        var bitmap = new BitmapImage();
        _ = bitmap.SetSourceAsync(stream);
        return bitmap;
    }

    /// <summary>A transient failure must not become a permanently blank icon.</summary>
    private static async void EvictFailure(string path, Task<ImageSource?> task)
    {
        if (await task is null
            && Cache.TryGet(path, out var current)
            && current == task)
        {
            Cache.TryRemove(path);
        }
    }

    private static async Task<ImageSource?> RenderAsync(string path)
    {
        try
        {
            var bitmap = await Task.Run(() => RenderBitmap(path));
            if (bitmap is null)
            {
                return null;
            }

            var source = new SoftwareBitmapSource();
            await source.SetBitmapAsync(bitmap);
            return source;
        }
        catch
        {
            return null;
        }
    }

    private static SoftwareBitmap? RenderBitmap(string path)
    {
        nint hbitmap = 0;
        try
        {
            if (SHCreateItemFromParsingName(
                    path, 0, typeof(IShellItemImageFactory).GUID, out var factory) != 0)
            {
                return null;
            }

            factory.GetImage(new Size(RenderSize, RenderSize), 0, out hbitmap);
            return hbitmap == 0 ? null : FromHBitmap(new HBITMAP(hbitmap));
        }
        catch
        {
            return null;
        }
        finally
        {
            if (hbitmap != 0)
            {
                _ = PInvoke.DeleteObject(new HGDIOBJ(hbitmap));
            }
        }
    }

    private static unsafe SoftwareBitmap? FromHBitmap(HBITMAP hbitmap)
    {
        BITMAP bitmap;
        if (PInvoke.GetObject(hbitmap, sizeof(BITMAP), &bitmap) == 0
            || bitmap.bmBitsPixel != 32)
        {
            return null;
        }

        var info = new BITMAPINFO
        {
            bmiHeader = new BITMAPINFOHEADER
            {
                biSize = (uint)sizeof(BITMAPINFOHEADER),
                biWidth = bitmap.bmWidth,
                biHeight = -bitmap.bmHeight, // top-down rows
                biPlanes = 1,
                biBitCount = 32,
            },
        };
        var pixels = new byte[bitmap.bmWidth * bitmap.bmHeight * 4];
        var hdc = PInvoke.GetDC(HWND.Null);
        try
        {
            fixed (byte* bits = pixels)
            {
                if (PInvoke.GetDIBits(
                        hdc,
                        hbitmap,
                        0,
                        (uint)bitmap.bmHeight,
                        bits,
                        &info,
                        DIB_USAGE.DIB_RGB_COLORS) == 0)
                {
                    return null;
                }
            }
        }
        finally
        {
            _ = PInvoke.ReleaseDC(HWND.Null, hdc);
        }

        using var straight = SoftwareBitmap.CreateCopyFromBuffer(
            pixels.AsBuffer(),
            BitmapPixelFormat.Bgra8,
            bitmap.bmWidth,
            bitmap.bmHeight,
            BitmapAlphaMode.Straight);
        return SoftwareBitmap.Convert(
            straight,
            BitmapPixelFormat.Bgra8,
            BitmapAlphaMode.Premultiplied);
    }

    // CsWin32 would generate this interface with the legacy ComImport
    // pattern, whose runtime-built marshaling breaks Native AOT. Writing
    // it by hand keeps the compile-time [GeneratedComInterface] path.
    [GeneratedComInterface]
    [Guid("bcc18b79-ba16-442f-80c4-8a59c30c463b")]
    internal partial interface IShellItemImageFactory
    {
        void GetImage(Size size, uint flags, out nint bitmap);
    }

    [StructLayout(LayoutKind.Sequential)]
    internal struct Size(int width, int height)
    {
        public int Width = width;
        public int Height = height;
    }

    [LibraryImport("shell32.dll", StringMarshalling = StringMarshalling.Utf16)]
    private static partial int SHCreateItemFromParsingName(
        string path, nint bindContext, in Guid riid, out IShellItemImageFactory factory);
}
