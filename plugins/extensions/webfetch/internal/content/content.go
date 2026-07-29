// Package content turns a fetched body into the text handed to the model.
package content

import (
	"bytes"
	"errors"
	"fmt"
	"mime"
	"net/http"
	"strings"
	"unicode/utf8"

	"github.com/JohannesKaufmann/dom"
	"github.com/JohannesKaufmann/html-to-markdown/v2/converter"
	"github.com/JohannesKaufmann/html-to-markdown/v2/plugin/base"
	"github.com/JohannesKaufmann/html-to-markdown/v2/plugin/commonmark"
	"github.com/JohannesKaufmann/html-to-markdown/v2/plugin/strikethrough"
	"github.com/JohannesKaufmann/html-to-markdown/v2/plugin/table"
	"golang.org/x/net/html"
	"golang.org/x/net/html/charset"
	"golang.org/x/text/encoding/unicode"
	"golang.org/x/text/transform"
)

// Format selects how an HTML body is rendered.
type Format string

const (
	Markdown Format = "markdown"
	Text     Format = "text"
	HTML     Format = "html"
)

// ErrBinary is returned when the body's media type has no textual rendering.
var ErrBinary = errors.New("binary content")

// Rendered is Render's outcome. MediaType is populated even when rendering
// fails, so errors can name what was actually fetched.
type Rendered struct {
	Text string
	// effective media type without parameters: from the Content-Type header
	// when present, sniffed from the body otherwise
	MediaType string
	// Format is what Text actually is — it diverges from the requested
	// format on passthrough bodies and degraded conversions, and the model
	// must be told what it is parsing
	Format Format
	// Degraded carries the conversion failure when Text holds a fallback
	// rendering instead of the requested one
	Degraded error
}

func ParseFormat(raw string) (Format, bool) {
	switch Format(strings.ToLower(strings.TrimSpace(raw))) {
	case "", Markdown:
		return Markdown, true
	case Text:
		return Text, true
	case HTML:
		return HTML, true
	default:
		return "", false
	}
}

// Render converts body according to format. Non-HTML textual bodies pass
// through regardless of format; binary bodies are refused with ErrBinary —
// the only error Render returns. A failed HTML conversion degrades to the
// best available fallback, reported on Rendered.Degraded rather than as an
// error, so callers never have to decide whether a non-nil error still
// carries usable Text.
func Render(body []byte, contentType string, format Format) (Rendered, error) {
	rendered := Rendered{MediaType: effectiveMediaType(contentType, body)}

	if !textual(rendered.MediaType) {
		return rendered, fmt.Errorf("%w: %s", ErrBinary, rendered.MediaType)
	}

	text := decode(body, contentType)
	switch {
	case format == HTML:
		rendered.Text, rendered.Format = text, HTML
	case !isHTML(rendered.MediaType):
		// nothing to convert: the decoded source is plain text
		rendered.Text, rendered.Format = text, Text
	default:
		rendered.Text, rendered.Format, rendered.Degraded = renderHTML(text, format)
	}
	return rendered, nil
}

// renderHTML converts HTML source to the requested format, degrading to
// plain text and then to the raw source when a converter fails.
func renderHTML(source string, format Format) (string, Format, error) {
	if format == Text {
		extracted, err := extractText(source)
		if err != nil {
			return source, HTML, fmt.Errorf("extracting text: %w", err)
		}
		return extracted, Text, nil
	}

	converted, err := convertMarkdown(source)
	if err == nil {
		return converted, Markdown, nil
	}
	err = fmt.Errorf("converting to markdown: %w", err)
	if extracted, textErr := extractText(source); textErr == nil {
		return extracted, Text, err
	}
	return source, HTML, err
}

// effectiveMediaType trusts a Content-Type header that commits to a type; a
// missing, malformed, or application/octet-stream header falls back to
// sniffing, which rescues the many servers that serve text files as
// octet-stream.
func effectiveMediaType(contentType string, body []byte) string {
	mediaType, _, err := mime.ParseMediaType(contentType)
	if err == nil && mediaType != "application/octet-stream" {
		return mediaType
	}

	sniffed, _, sniffErr := mime.ParseMediaType(http.DetectContentType(body))
	if sniffErr != nil {
		// DetectContentType always returns a valid type; this is unreachable
		// in practice but octet-stream is the honest answer if it happens
		return "application/octet-stream"
	}
	return sniffed
}

func isHTML(mediaType string) bool {
	return mediaType == "text/html" || mediaType == "application/xhtml+xml"
}

func textual(mediaType string) bool {
	if strings.HasPrefix(mediaType, "text/") {
		return true
	}
	switch mediaType {
	case "application/json", "application/xml", "application/javascript",
		"application/ecmascript", "application/x-ndjson",
		"application/x-www-form-urlencoded", "application/xhtml+xml":
		return true
	}
	return strings.HasSuffix(mediaType, "+json") || strings.HasSuffix(mediaType, "+xml")
}

// decode converts body to UTF-8, honoring a charset declared via header
// parameter, BOM, or <meta> tag. Undeclared valid UTF-8 is kept as-is:
// DetermineEncoding's windows-1252 default would otherwise mangle most of
// the modern web.
func decode(body []byte, contentType string) string {
	enc, name, certain := charset.DetermineEncoding(body, contentType)
	// Valid UTF-8 — declared, sniffed, or hiding behind the uncertain
	// windows-1252 default — is returned without the decoder's full-copy
	// transform. trimIncompleteRune keeps a multi-byte rune chopped by the
	// size cap from disqualifying an otherwise valid body, which would
	// mojibake all of it through the windows-1252 fallback.
	if name == "utf-8" || (!certain && name == "windows-1252") {
		trimmed := trimIncompleteRune(bytes.TrimPrefix(body, bomUTF8))
		if utf8.Valid(trimmed) {
			return string(trimmed)
		}
	}

	decoded, _, err := transform.Bytes(unicode.BOMOverride(enc.NewDecoder()), body)
	if err != nil {
		decoded = body
	}
	return sanitizeUTF8(string(decoded))
}

var bomUTF8 = []byte("\ufeff")

// trimIncompleteRune drops a trailing UTF-8 sequence cut short by
// truncation. Anything else — including genuinely invalid bytes — is left
// alone for the decoder path to judge.
func trimIncompleteRune(body []byte) []byte {
	for i := 1; i <= utf8.UTFMax && i <= len(body); i++ {
		start := len(body) - i
		if !utf8.RuneStart(body[start]) {
			continue
		}
		if !utf8.FullRune(body[start:]) {
			return body[:start]
		}
		return body
	}
	return body
}

// sanitizeUTF8 closes decode's contract that its result is valid UTF-8,
// which the renderers and everything downstream assume.
func sanitizeUTF8(s string) string {
	if utf8.ValidString(s) {
		return s
	}
	return strings.ToValidUTF8(s, "�")
}

// The top-level htmltomarkdown.ConvertString registers only base+commonmark,
// which flattens tables into glued cell text and drops strikethrough; the
// library ships plugins for both.
func convertMarkdown(source string) (string, error) {
	conv := converter.NewConverter(converter.WithPlugins(
		base.NewBasePlugin(),
		commonmark.NewCommonmarkPlugin(),
		table.NewTablePlugin(),
		strikethrough.NewStrikethroughPlugin(),
	))
	return conv.ConvertString(source)
}

// All markdown syntax comes from the commonmark plugin, so the base plugin
// alone renders plain text while keeping the library's tag knowledge — which
// subtrees are dropped (script, style, head, ...) and how blocks are spaced
// stay identical to the markdown path. Escaping only exists to protect
// markdown syntax, so it is disabled.
func extractText(source string) (string, error) {
	conv := converter.NewConverter(
		converter.WithPlugins(base.NewBasePlugin()),
		converter.WithEscapeMode(converter.EscapeModeDisabled),
	)
	conv.Register.Renderer(base.RenderAsPlaintextWrapper, converter.PriorityStandard)
	// the defaults render <br> as nothing and give table rows and cells no
	// separation, gluing adjacent words together
	conv.Register.RendererFor("br", converter.TagTypeInline, renderLineBreak, converter.PriorityEarly)
	conv.Register.RendererFor("td", converter.TagTypeInline, renderCell, converter.PriorityEarly)
	conv.Register.RendererFor("th", converter.TagTypeInline, renderCell, converter.PriorityEarly)
	conv.Register.RendererFor("tr", converter.TagTypeBlock, base.RenderAsPlaintextWrapper, converter.PriorityEarly)

	return conv.ConvertString(source)
}

// the Writer is a bytes.Buffer, whose writes always return a nil error, and
// a RenderStatus is the only thing a renderer can propagate anyway
func renderLineBreak(_ converter.Context, w converter.Writer, _ *html.Node) converter.RenderStatus {
	_, _ = w.WriteString("\n")
	return converter.RenderSuccess
}

func renderCell(ctx converter.Context, w converter.Writer, node *html.Node) converter.RenderStatus {
	ctx.RenderChildNodes(ctx, w, node)
	if dom.NextSiblingElement(node) != nil {
		_, _ = w.WriteString(" ")
	}
	return converter.RenderSuccess
}
