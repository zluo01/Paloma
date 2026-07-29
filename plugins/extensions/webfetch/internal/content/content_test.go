package content

import (
	"errors"
	"strings"
	"testing"
)

func TestParseFormat(t *testing.T) {
	cases := map[string]Format{
		"":         Markdown,
		"markdown": Markdown,
		"MarkDown": Markdown,
		" text ":   Text,
		"html":     HTML,
	}
	for raw, want := range cases {
		got, ok := ParseFormat(raw)
		if !ok || got != want {
			t.Errorf("ParseFormat(%q) = %q, %v; want %q, true", raw, got, ok, want)
		}
	}

	if _, ok := ParseFormat("pdf"); ok {
		t.Error("ParseFormat(\"pdf\") accepted an unknown format")
	}
}

func TestRenderConvertsHTMLToMarkdown(t *testing.T) {
	html := `<h1>Title</h1><p>Some <strong>bold</strong> text and a <a href="https://example.com">link</a>.</p>`

	got, err := Render([]byte(html), "text/html; charset=utf-8", Markdown)
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	for _, want := range []string{"# Title", "**bold**", "[link](https://example.com)"} {
		if !strings.Contains(got.Text, want) {
			t.Errorf("markdown missing %q, got:\n%s", want, got.Text)
		}
	}
	if strings.Contains(got.Text, "<h1>") {
		t.Errorf("html tags survived conversion:\n%s", got.Text)
	}
	if got.MediaType != "text/html" {
		t.Errorf("media type = %q, want text/html", got.MediaType)
	}
}

func TestRenderHTMLFormatReturnsSource(t *testing.T) {
	html := `<h1>Title</h1>`

	got, err := Render([]byte(html), "text/html", HTML)
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	if got.Text != html {
		t.Errorf("got %q, want the untouched source", got.Text)
	}
}

func TestRenderTextFormatExtractsPlainText(t *testing.T) {
	html := `<html><head><title>Page</title><style>body { color: red }</style></head>
	<body><h1>Title</h1><p>Some <strong>bold</strong> text and a
	<a href="https://example.com">link</a>.</p><script>alert(1)</script></body></html>`

	got, err := Render([]byte(html), "text/html", Text)
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	for _, want := range []string{"Title", "bold", "link"} {
		if !strings.Contains(got.Text, want) {
			t.Errorf("text missing %q, got:\n%s", want, got.Text)
		}
	}
	for _, leak := range []string{"**", "](", "<h1>", "alert(1)", "color: red"} {
		if strings.Contains(got.Text, leak) {
			t.Errorf("text format leaked %q:\n%s", leak, got.Text)
		}
	}
}

func TestRenderTextFormatBreaksAtBlockBoundaries(t *testing.T) {
	html := `<p>first</p><p>second</p>`

	got, err := Render([]byte(html), "text/html", Text)
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	if !strings.Contains(got.Text, "first\n\nsecond") {
		t.Errorf("block elements did not break lines:\n%q", got.Text)
	}
}

func TestRenderTextFormatSeparatesCellsAndLineBreaks(t *testing.T) {
	html := `<table><tr><td>alpha</td><td>beta</td></tr><tr><td>gamma</td><td>delta</td></tr></table>
	<p>line<br>break</p>`

	got, err := Render([]byte(html), "text/html", Text)
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	for _, want := range []string{"alpha beta", "gamma delta", "line\nbreak"} {
		if !strings.Contains(got.Text, want) {
			t.Errorf("text missing %q, got:\n%q", want, got.Text)
		}
	}
	if strings.Contains(got.Text, "alpha beta gamma") {
		t.Errorf("table rows ran together:\n%q", got.Text)
	}
	if strings.Contains(got.Text, " \n") || strings.HasSuffix(got.Text, " ") {
		t.Errorf("trailing whitespace survived:\n%q", got.Text)
	}
}

func TestRenderPassesThroughNonHTML(t *testing.T) {
	body := `{"key": "value"}`

	got, err := Render([]byte(body), "application/json", Markdown)
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	if got.Text != body {
		t.Errorf("got %q, want the body unchanged", got.Text)
	}
	if got.MediaType != "application/json" {
		t.Errorf("media type = %q, want application/json", got.MediaType)
	}
}

func TestRenderRefusesBinaryContent(t *testing.T) {
	png := append([]byte("\x89PNG\r\n\x1a\n"), make([]byte, 64)...)

	got, err := Render(png, "image/png", Markdown)

	if !errors.Is(err, ErrBinary) {
		t.Fatalf("err = %v, want ErrBinary", err)
	}
	if got.MediaType != "image/png" {
		t.Errorf("media type = %q, want image/png", got.MediaType)
	}
	if got.Text != "" {
		t.Errorf("binary content produced text: %q", got.Text)
	}
}

func TestRenderSniffsBinaryWithoutHeader(t *testing.T) {
	png := append([]byte("\x89PNG\r\n\x1a\n"), make([]byte, 64)...)

	_, err := Render(png, "", Markdown)

	if !errors.Is(err, ErrBinary) {
		t.Fatalf("err = %v, want ErrBinary from sniffed png", err)
	}
}

func TestRenderSniffsHTMLWithoutHeader(t *testing.T) {
	html := `<!DOCTYPE html><html><body><h1>Hello</h1></body></html>`

	got, err := Render([]byte(html), "", Markdown)
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	if !strings.Contains(got.Text, "# Hello") {
		t.Errorf("sniffed html was not converted:\n%s", got.Text)
	}
	if got.MediaType != "text/html" {
		t.Errorf("media type = %q, want text/html", got.MediaType)
	}
}

func TestRenderRescuesTextServedAsOctetStream(t *testing.T) {
	body := "#!/bin/sh\necho hello\n"

	got, err := Render([]byte(body), "application/octet-stream", Markdown)
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	if !strings.Contains(got.Text, "echo hello") {
		t.Errorf("text served as octet-stream was lost: %q", got.Text)
	}
}

func TestRenderDecodesHeaderDeclaredCharset(t *testing.T) {
	body := []byte("<p>caf\xe9</p>") // é in ISO-8859-1

	got, err := Render(body, "text/html; charset=iso-8859-1", Markdown)
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	if !strings.Contains(got.Text, "café") {
		t.Errorf("iso-8859-1 body not decoded: %q", got.Text)
	}
}

func TestRenderDecodesMetaDeclaredCharset(t *testing.T) {
	body := []byte(`<html><head><meta charset="iso-8859-1"></head><body>caf` + "\xe9" + `</body></html>`)

	got, err := Render(body, "text/html", Text)
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	if !strings.Contains(got.Text, "café") {
		t.Errorf("meta-declared charset not honored: %q", got.Text)
	}
}

func TestRenderKeepsUndeclaredUTF8(t *testing.T) {
	body := "café über 東京" // valid UTF-8, no charset declared anywhere

	got, err := Render([]byte(body), "text/plain", Markdown)
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	if got.Text != body {
		t.Errorf("undeclared utf-8 was mangled: %q", got.Text)
	}
}

func TestRenderKeepsUTF8CutMidRune(t *testing.T) {
	// non-ASCII only past DetermineEncoding's 1024-byte sniff window, so the
	// undeclared-UTF-8 keep is the only thing standing between this body and
	// a windows-1252 fallback
	full := append([]byte(strings.Repeat("ascii padding ", 100)), "café and 東京"...)
	cut := full[:len(full)-1] // slice through 京's final byte, as a size cap would

	got, err := Render(cut, "text/plain", Markdown)
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	if !strings.Contains(got.Text, "café") || !strings.Contains(got.Text, "東") {
		t.Errorf("intact runes were mojibaked: %q", got.Text)
	}
	if strings.Contains(got.Text, "Ã©") {
		t.Errorf("body fell back to windows-1252: %q", got.Text)
	}
}

func TestRenderDecodesUTF16WithBOM(t *testing.T) {
	body := []byte{0xff, 0xfe, 'h', 0, 'i', 0} // "hi" in UTF-16LE with BOM

	got, err := Render(body, "text/plain", Markdown)
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	if !strings.Contains(got.Text, "hi") {
		t.Errorf("utf-16le body not decoded: %q", got.Text)
	}
	if strings.ContainsRune(got.Text, '\ufeff') {
		t.Errorf("BOM survived decoding: %q", got.Text)
	}
}

func TestRenderReplacesInvalidDeclaredUTF8(t *testing.T) {
	body := []byte{'o', 'k', ' ', 0xff, 0xf0}

	got, err := Render(body, "text/plain; charset=utf-8", Markdown)
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	if !strings.Contains(got.Text, "ok") {
		t.Errorf("valid bytes lost: %q", got.Text)
	}
	if !strings.ContainsRune(got.Text, 0xfffd) {
		t.Errorf("invalid bytes not replaced: %q", got.Text)
	}
}

func TestTextualCoversCommonTypes(t *testing.T) {
	textualTypes := []string{
		"text/html", "text/plain", "text/csv", "application/json",
		"application/xml", "application/javascript", "image/svg+xml",
		"application/ld+json", "application/xhtml+xml",
	}
	for _, mediaType := range textualTypes {
		if !textual(mediaType) {
			t.Errorf("textual(%q) = false, want true", mediaType)
		}
	}

	binaryTypes := []string{
		"image/png", "application/pdf", "application/octet-stream",
		"application/zip", "audio/mpeg", "video/mp4",
	}
	for _, mediaType := range binaryTypes {
		if textual(mediaType) {
			t.Errorf("textual(%q) = true, want false", mediaType)
		}
	}
}

func TestRenderStripsUTF8BOM(t *testing.T) {
	body := append([]byte{0xef, 0xbb, 0xbf}, []byte("hello")...)

	got, err := Render(body, "text/plain", Markdown)
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	if got.Text != "hello" {
		t.Errorf("text = %q, want the BOM stripped", got.Text)
	}
}

func TestRenderNonHTMLIgnoresFormat(t *testing.T) {
	body := `{"key": "value"}`

	for _, format := range []Format{Text, HTML} {
		got, err := Render([]byte(body), "application/json", format)
		if err != nil {
			t.Fatalf("render %s: %v", format, err)
		}
		if got.Text != body {
			t.Errorf("format %s changed a non-HTML body: %q", format, got.Text)
		}
	}
}

func TestRenderEmptyBody(t *testing.T) {
	got, err := Render(nil, "", Markdown)
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	if got.Text != "" {
		t.Errorf("text = %q, want empty", got.Text)
	}
	if got.MediaType != "text/plain" {
		t.Errorf("media type = %q, want the sniffed text/plain", got.MediaType)
	}
}
