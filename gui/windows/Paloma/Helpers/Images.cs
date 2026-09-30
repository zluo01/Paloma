using System.Runtime.InteropServices;
using System.Runtime.InteropServices.WindowsRuntime;
using Windows.Graphics.Imaging;
using Windows.Storage;
using Windows.Storage.Streams;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using Paloma.Models;
using Serilog;

namespace Paloma.Helpers;

internal static class Images
{
    public const char ImageCharacter = '\uFFFC';

    private const long MaxBytes = 20 * 1024 * 1024;

    private const string PngMediaType = "image/png";
    private const double MaxThumbnailAspect = 3;

    private static readonly HashSet<string> SendableMediaTypes = ["image/png", "image/jpeg", "image/gif", "image/webp"];

    public static async Task<Loaded?> LoadFileAsync(StorageFile file, double lineHeight, double scale)
    {
        if (!SendableMediaTypes.Contains(file.ContentType))
        {
            return null;
        }

        try
        {
            var properties = await file.GetBasicPropertiesAsync();
            if (properties.Size > MaxBytes)
            {
                return null;
            }

            using var source = await file.OpenReadAsync();
            var decoder = await CreateDecoderAsync(source);
            if (decoder == null)
            {
                return null;
            }

            var (transform, boxWidth) = ThumbnailTransform(decoder.OrientedPixelWidth, decoder.OrientedPixelHeight, lineHeight, scale);
            if (decoder.PixelWidth != decoder.OrientedPixelWidth)
            {
                (transform.ScaledWidth, transform.ScaledHeight) = (transform.ScaledHeight, transform.ScaledWidth);
            }

            using var thumbnail = await DecodeAsync(decoder, transform);
            return new Loaded(
                new InlineImage(file.ContentType, await ReadImageAsync(source)),
                boxWidth,
                (int)lineHeight,
                await EncodePngAsync(thumbnail, new BitmapTransform()));
        }
        catch (Exception e)
        {
            Log.Error(e, "fail to load image {Path}, fallback to file path", file.Path);
            return null;
        }
    }

    public static async Task<Loaded?> LoadBitmapAsync(RandomAccessStreamReference reference, double lineHeight, double scale)
    {
        try
        {
            using var source = await reference.OpenReadAsync();
            var decoder = await CreateDecoderAsync(source);
            if (decoder == null)
            {
                return null;
            }

            using var bitmap = await DecodeAsync(decoder, new BitmapTransform());
            using var png = await EncodePngAsync(bitmap, new BitmapTransform());
            if (png.Size > MaxBytes)
            {
                return null;
            }

            var (transform, boxWidth) = ThumbnailTransform((uint)bitmap.PixelWidth, (uint)bitmap.PixelHeight, lineHeight, scale);
            return new Loaded(
                new InlineImage(PngMediaType, await ReadImageAsync(png)),
                boxWidth,
                (int)lineHeight,
                await EncodePngAsync(bitmap, transform));
        }
        catch (Exception e)
        {
            Log.Error(e, "fail to paste image from clipboard");
            return null;
        }
    }

    private static async Task<BitmapDecoder?> CreateDecoderAsync(IRandomAccessStream source)
    {
        try
        {
            return await BitmapDecoder.CreateAsync(source);
        }
        catch (COMException)
        {
            return null;
        }
    }

    private static async Task<SoftwareBitmap> DecodeAsync(BitmapDecoder decoder, BitmapTransform transform)
    {
        return await decoder.GetSoftwareBitmapAsync(
            BitmapPixelFormat.Bgra8,
            BitmapAlphaMode.Premultiplied,
            transform,
            ExifOrientationMode.RespectExifOrientation,
            ColorManagementMode.ColorManageToSRgb);
    }

    // aspect ratio scale along the line height, center-cropped past MaxThumbnailAspect
    private static (BitmapTransform Transform, int BoxWidth) ThumbnailTransform(uint width, uint height, double lineHeight, double scale)
    {
        var aspect = Math.Clamp((double)width / height, 1 / MaxThumbnailAspect, MaxThumbnailAspect);
        var boxWidth = Math.Max(1, Math.Round(lineHeight * aspect));
        var thumbnailWidth = (uint)Math.Round(boxWidth * scale);
        var thumbnailHeight = (uint)Math.Round(lineHeight * scale);
        var cover = Math.Max((double)thumbnailWidth / width, (double)thumbnailHeight / height);
        var scaledWidth = Math.Max(thumbnailWidth, (uint)Math.Round(width * cover));
        var scaledHeight = Math.Max(thumbnailHeight, (uint)Math.Round(height * cover));
        var transform = new BitmapTransform
        {
            ScaledWidth = scaledWidth,
            ScaledHeight = scaledHeight,
            Bounds = new BitmapBounds
            {
                X = (scaledWidth - thumbnailWidth) / 2,
                Y = (scaledHeight - thumbnailHeight) / 2,
                Width = thumbnailWidth,
                Height = thumbnailHeight,
            },
            InterpolationMode = BitmapInterpolationMode.Fant,
        };
        return (transform, (int)boxWidth);
    }

    private static async Task<InMemoryRandomAccessStream> EncodePngAsync(SoftwareBitmap bitmap, BitmapTransform transform)
    {
        var stream = new InMemoryRandomAccessStream();
        var encoder = await BitmapEncoder.CreateAsync(BitmapEncoder.PngEncoderId, stream);
        encoder.SetSoftwareBitmap(bitmap);
        encoder.BitmapTransform.ScaledWidth = transform.ScaledWidth;
        encoder.BitmapTransform.ScaledHeight = transform.ScaledHeight;
        encoder.BitmapTransform.Bounds = transform.Bounds;
        encoder.BitmapTransform.InterpolationMode = transform.InterpolationMode;
        await encoder.FlushAsync();
        stream.Seek(0);
        return stream;
    }

    private static async Task<byte[]> ReadImageAsync(IRandomAccessStream stream)
    {
        var data = new byte[stream.Size];
        stream.Seek(0);
        await stream.ReadAsync(data.AsBuffer(), (uint)data.Length, InputStreamOptions.None);
        return data;
    }

    public sealed record Loaded(InlineImage Image, int Width, int Height, InMemoryRandomAccessStream Thumbnail);

    public static Image Picture(byte[] data, double height)
    {
        var bitmap = new BitmapImage { DecodePixelHeight = (int)height, DecodePixelType = DecodePixelType.Logical };
        _ = LoadAsync(bitmap, data);
        return new Image { MaxHeight = height, Stretch = Stretch.Uniform, Source = bitmap };
    }

    private static async Task LoadAsync(BitmapImage bitmap, byte[] data)
    {
        try
        {
            using var stream = new InMemoryRandomAccessStream();
            await stream.WriteAsync(data.AsBuffer());
            stream.Seek(0);
            await bitmap.SetSourceAsync(stream);
        }
        catch (Exception e)
        {
            Log.Error(e, "fail to show a prompt image");
        }
    }
}
