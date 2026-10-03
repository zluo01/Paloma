using System.Runtime.InteropServices.WindowsRuntime;
using System.Text;
using Windows.Foundation;
using Windows.Graphics.Imaging;
using Windows.Storage;
using Windows.Storage.Streams;
using Paloma.Helpers;
using Xunit;
using Buffer = Windows.Storage.Streams.Buffer;

namespace Paloma.Tests;

public sealed class ImagesTests
{
    private const double LineHeight = 27;

    private static readonly byte[] Webp = Convert.FromBase64String("UklGRhoAAABXRUJQVlA4TA0AAAAvAAAAEAcQERGIiP4HAA==");

    // There is no Webp Decoder on windows server, skip when running under CI/CD
    private static bool HasWebpDecoder =>
        BitmapDecoder.GetDecoderInformationEnumerator().Any(codec => codec.CodecId == BitmapDecoder.WebpDecoderId);

    private static readonly Dictionary<string, Guid> Encoders = new()
    {
        ["png"] = BitmapEncoder.PngEncoderId,
        ["jpeg"] = BitmapEncoder.JpegEncoderId,
        ["gif"] = BitmapEncoder.GifEncoderId,
        ["bmp"] = BitmapEncoder.BmpEncoderId,
        ["tiff"] = BitmapEncoder.TiffEncoderId,
    };

    [Theory]
    [InlineData("png", "image/png")]
    [InlineData("jpeg", "image/jpeg")]
    [InlineData("gif", "image/gif")]
    public async Task GivenSendableFormatWhenLoadingFileShouldKeepItsBytes(string format, string mediaType)
    {
        var bytes = await EncodeAsync(format, 64, 32, Halves);

        var loaded = await LoadFileAsync(format, bytes, LineHeight, 1);

        Assert.NotNull(loaded);
        Assert.Equal(mediaType, loaded.Image.MediaType);
        Assert.Equal(bytes, loaded.Image.Data);
        Assert.Equal(54, loaded.Width);
        Assert.Equal(27, loaded.Height);
    }

    [Fact]
    public async Task GivenWebpWhenLoadingFileShouldKeepItsBytes()
    {
        Assert.SkipUnless(HasWebpDecoder, "No WebP decoder is registered on this machine");

        var loaded = await LoadFileAsync("webp", Webp, LineHeight, 1);

        Assert.NotNull(loaded);
        Assert.Equal("image/webp", loaded.Image.MediaType);
        Assert.Equal(Webp, loaded.Image.Data);
        Assert.Equal(27, loaded.Width);
        Assert.Equal(27, loaded.Height);
    }

    [Theory]
    [InlineData("bmp")]
    [InlineData("tiff")]
    public async Task GivenUnsupportedImageFormatWhenLoadingFileShouldReturnNull(string format)
    {
        var bytes = await EncodeAsync(format, 64, 32, Halves);

        Assert.Null(await LoadFileAsync(format, bytes, LineHeight, 1));
    }

    [Fact]
    public async Task GivenPngNamedTxtWhenLoadingFileShouldReturnNull()
    {
        var bytes = await EncodeAsync("png", 64, 32, Halves);

        Assert.Null(await LoadFileAsync("txt", bytes, LineHeight, 1));
    }

    [Theory]
    [InlineData("txt", "text")]
    [InlineData("png", "corrupt")]
    [InlineData("png", "empty")]
    public async Task GivenNonImageWhenLoadingFileShouldReturnNull(string extension, string kind)
    {
        var bytes = kind switch
        {
            "text" => Encoding.UTF8.GetBytes("hello"),
            "corrupt" => [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0],
            _ => [],
        };

        Assert.Null(await LoadFileAsync(extension, bytes, LineHeight, 1));
    }

    [Fact]
    public async Task GivenFileOverSizeLimitWhenLoadingFileShouldReturnNull()
    {
        var bytes = await EncodeAsync("png", 64, 32, Halves);
        Array.Resize(ref bytes, 20 * 1024 * 1024 + 1);

        Assert.Null(await LoadFileAsync("png", bytes, LineHeight, 1));
    }

    [Theory]
    [InlineData(100u, 100u, 27, 1.0, 27, 27, 27u, 27u)]
    [InlineData(200u, 100u, 27, 1.5, 54, 27, 81u, 40u)]
    [InlineData(100u, 200u, 27, 1.0, 14, 27, 14u, 27u)]
    [InlineData(160u, 90u, 30, 2.0, 53, 30, 106u, 60u)]
    [InlineData(1u, 1u, 27, 1.25, 27, 27, 34u, 34u)]
    [InlineData(1000u, 10u, 20, 1.0, 60, 20, 60u, 20u)]
    [InlineData(10u, 1000u, 20, 1.0, 7, 20, 7u, 20u)]
    public async Task GivenImageSizeWhenLoadingFileShouldScaleThumbnailToLineHeight(
        uint width, uint height, double lineHeight, double scale,
        int boxWidth, int boxHeight, uint pixelWidth, uint pixelHeight)
    {
        var bytes = await EncodeAsync("png", width, height, Halves);
        var loaded = await LoadFileAsync("png", bytes, lineHeight, scale);

        Assert.NotNull(loaded);
        Assert.Equal(boxWidth, loaded.Width);
        Assert.Equal(boxHeight, loaded.Height);
        var thumbnail = await BitmapDecoder.CreateAsync(loaded.Thumbnail);
        Assert.Equal(pixelWidth, thumbnail.PixelWidth);
        Assert.Equal(pixelHeight, thumbnail.PixelHeight);
    }

    [Theory]
    [InlineData(300u, 30u, 81)]
    [InlineData(30u, 300u, 9)]
    public async Task GivenAspectPastCapWhenLoadingFileShouldCropTheMiddle(uint width, uint height, int boxWidth)
    {
        var bytes = await EncodeAsync("png", width, height, Thirds);

        var loaded = await LoadFileAsync("png", bytes, LineHeight, 1);

        Assert.NotNull(loaded);
        Assert.Equal(boxWidth, loaded.Width);
        var thumbnail = await BitmapDecoder.CreateAsync(loaded.Thumbnail);
        Assert.Equal(["green", "green", "green", "green"], await QuadrantsAsync(thumbnail, ExifOrientationMode.IgnoreExifOrientation));
    }

    [Theory]
    [InlineData((ushort)1, 81, 122u, 40u)]
    [InlineData((ushort)2, 81, 122u, 40u)]
    [InlineData((ushort)3, 81, 122u, 40u)]
    [InlineData((ushort)4, 81, 122u, 40u)]
    [InlineData((ushort)5, 9, 14u, 40u)]
    [InlineData((ushort)6, 9, 14u, 40u)]
    [InlineData((ushort)7, 9, 14u, 40u)]
    [InlineData((ushort)8, 9, 14u, 40u)]
    public async Task GivenExifOrientationWhenLoadingFileShouldTurnThumbnailUpright(ushort orientation, int boxWidth, uint pixelWidth, uint pixelHeight)
    {
        var bytes = await EncodeAsync("jpeg", 120, 40, Grid, orientation);

        var loaded = await LoadFileAsync("jpeg", bytes, LineHeight, 1.5);

        Assert.NotNull(loaded);
        Assert.Equal(bytes, loaded.Image.Data);
        Assert.Equal(boxWidth, loaded.Width);
        Assert.Equal(27, loaded.Height);
        var thumbnail = await BitmapDecoder.CreateAsync(loaded.Thumbnail);
        Assert.Equal(pixelWidth, thumbnail.PixelWidth);
        Assert.Equal(pixelHeight, thumbnail.PixelHeight);
        Assert.Equal(await QuadrantsAsync(await DecodeAsync(bytes), ExifOrientationMode.RespectExifOrientation),
            await QuadrantsAsync(thumbnail, ExifOrientationMode.IgnoreExifOrientation));
    }

    [Theory]
    [InlineData("bmp")]
    [InlineData("png")]
    [InlineData("jpeg")]
    [InlineData("gif")]
    public async Task GivenImageWhenLoadingBitmapShouldEncodePng(string format)
    {
        var bytes = await EncodeAsync(format, 640, 400, Halves);

        var loaded = await Images.LoadBitmapAsync(Reference(bytes), LineHeight, 1);

        Assert.NotNull(loaded);
        Assert.Equal("image/png", loaded.Image.MediaType);
        var decoder = await DecodeAsync(loaded.Image.Data);
        Assert.Equal(BitmapDecoder.PngDecoderId, decoder.DecoderInformation.CodecId);
        Assert.Equal(640u, decoder.PixelWidth);
        Assert.Equal(400u, decoder.PixelHeight);
        Assert.Equal(43, loaded.Width);
        Assert.Equal(27, loaded.Height);
    }

    [Fact]
    public async Task GivenRotatedImageWhenLoadingBitmapShouldEncodeUpright()
    {
        var bytes = await EncodeAsync("jpeg", 120, 40, Halves, 6);

        var loaded = await Images.LoadBitmapAsync(Reference(bytes), LineHeight, 1);

        Assert.NotNull(loaded);
        var decoder = await DecodeAsync(loaded.Image.Data);
        Assert.Equal(40u, decoder.PixelWidth);
        Assert.Equal(120u, decoder.PixelHeight);
        Assert.Equal(await QuadrantsAsync(await DecodeAsync(bytes), ExifOrientationMode.RespectExifOrientation),
            await QuadrantsAsync(decoder, ExifOrientationMode.IgnoreExifOrientation));
    }

    [Fact]
    public async Task GivenBitmapOverSizeLimitWhenLoadingBitmapShouldReturnNull()
    {
        var bytes = await EncodeAsync("bmp", 2800, 2800, Noise);

        Assert.Null(await Images.LoadBitmapAsync(Reference(bytes), LineHeight, 1));
    }

    [Fact]
    public async Task GivenNonImageWhenLoadingBitmapShouldReturnNull()
    {
        Assert.Null(await Images.LoadBitmapAsync(Reference(Encoding.UTF8.GetBytes("hello")), LineHeight, 1));
    }

    private static string Halves(uint x, uint y, uint width, uint height)
    {
        if (x < width / 2)
        {
            return "red";
        }

        return "blue";
    }

    private static string Grid(uint x, uint y, uint width, uint height)
    {
        if (y < height / 2)
        {
            return x < width / 2 ? "red" : "green";
        }

        return x < width / 2 ? "blue" : "white";
    }

    private static string Noise(uint x, uint y, uint width, uint height)
    {
        return "noise";
    }

    private static string Thirds(uint x, uint y, uint width, uint height)
    {
        var position = x * 3 / width;
        if (height > width)
        {
            position = y * 3 / height;
        }

        if (position == 0)
        {
            return "red";
        }

        if (position == 1)
        {
            return "green";
        }

        return "blue";
    }

    private static async Task<byte[]> EncodeAsync(
        string format, uint width, uint height, Func<uint, uint, uint, uint, string> color, ushort orientation = 1)
    {
        var pixels = new byte[width * height * 4];
        var random = new Random(1);
        for (uint y = 0; y < height; y++)
        {
            for (uint x = 0; x < width; x++)
            {
                var i = (y * width + x) * 4;
                switch (color(x, y, width, height))
                {
                    case "red":
                        pixels[i + 2] = 255;
                        break;
                    case "green":
                        pixels[i + 1] = 255;
                        break;
                    case "white":
                        pixels[i] = 255;
                        pixels[i + 1] = 255;
                        pixels[i + 2] = 255;
                        break;
                    case "noise":
                        pixels[i] = (byte)random.Next(256);
                        pixels[i + 1] = (byte)random.Next(256);
                        pixels[i + 2] = (byte)random.Next(256);
                        break;
                    default:
                        pixels[i] = 255;
                        break;
                }

                pixels[i + 3] = 255;
            }
        }

        using var stream = new InMemoryRandomAccessStream();
        var encoder = await BitmapEncoder.CreateAsync(Encoders[format], stream);
        encoder.SetPixelData(BitmapPixelFormat.Bgra8, BitmapAlphaMode.Ignore, width, height, 96, 96, pixels);
        if (orientation != 1)
        {
            await encoder.BitmapProperties.SetPropertiesAsync(new BitmapPropertySet
            {
                ["System.Photo.Orientation"] = new BitmapTypedValue(orientation, PropertyType.UInt16),
            });
        }

        await encoder.FlushAsync();
        stream.Seek(0);
        var buffer = await stream.ReadAsync(new Buffer((uint)stream.Size), (uint)stream.Size, InputStreamOptions.None);
        return buffer.ToArray();
    }

    private static async Task<BitmapDecoder> DecodeAsync(byte[] bytes)
    {
        var stream = new InMemoryRandomAccessStream();
        await stream.WriteAsync(bytes.AsBuffer());
        stream.Seek(0);
        return await BitmapDecoder.CreateAsync(stream);
    }

    private static async Task<string[]> QuadrantsAsync(BitmapDecoder decoder, ExifOrientationMode orientation)
    {
        var data = await decoder.GetPixelDataAsync(
            BitmapPixelFormat.Bgra8,
            BitmapAlphaMode.Ignore,
            new BitmapTransform(),
            orientation,
            ColorManagementMode.DoNotColorManage);
        var pixels = data.DetachPixelData();
        var width = decoder.PixelWidth;
        var height = decoder.PixelHeight;
        if (orientation == ExifOrientationMode.RespectExifOrientation)
        {
            width = decoder.OrientedPixelWidth;
            height = decoder.OrientedPixelHeight;
        }

        string At(uint x, uint y)
        {
            var i = (y * width + x) * 4;
            if (pixels[i + 2] > 128 && pixels[i + 1] < 128 && pixels[i] < 128)
            {
                return "red";
            }

            if (pixels[i + 1] > 128 && pixels[i + 2] < 128 && pixels[i] < 128)
            {
                return "green";
            }

            if (pixels[i] > 128 && pixels[i + 1] > 128 && pixels[i + 2] > 128)
            {
                return "white";
            }

            if (pixels[i] > 128 && pixels[i + 2] < 128 && pixels[i + 1] < 128)
            {
                return "blue";
            }

            return $"({pixels[i + 2]},{pixels[i + 1]},{pixels[i]})";
        }

        return
        [
            At(width / 4, height / 4),
            At(width * 3 / 4, height / 4),
            At(width / 4, height * 3 / 4),
            At(width * 3 / 4, height * 3 / 4),
        ];
    }

    private static RandomAccessStreamReference Reference(byte[] bytes)
    {
        var stream = new InMemoryRandomAccessStream();
        stream.WriteAsync(bytes.AsBuffer()).AsTask().GetAwaiter().GetResult();
        stream.Seek(0);
        return RandomAccessStreamReference.CreateFromStream(stream);
    }

    private static async Task<Images.Loaded?> LoadFileAsync(string extension, byte[] bytes, double lineHeight, double scale)
    {
        var file = await StorageFile.CreateStreamedFileAsync($"image.{extension}", async request =>
        {
            using (request)
            {
                await request.WriteAsync(bytes.AsBuffer());
            }
        }, null);
        return await Images.LoadFileAsync(file, lineHeight, scale);
    }
}
