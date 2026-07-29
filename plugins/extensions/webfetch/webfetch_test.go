package main

import (
	"bytes"
	"context"
	"encoding/json"
	"strings"
	"testing"
	"time"
	"unicode/utf8"

	"scry/extensions/webfetch/internal/content"
	"scry/extensions/webfetch/internal/fetch"
	pb "scry/extensions/webfetch/internal/pb/schema/extension"
	"scry/extensions/webfetch/internal/transport"
)

// stubGetter returns a canned result and records the timeout it was handed.
type stubGetter struct {
	result  *fetch.Result
	err     error
	timeout time.Duration
}

func (s *stubGetter) Get(_ context.Context, _ string, timeout time.Duration) (*fetch.Result, error) {
	s.timeout = timeout
	return s.result, s.err
}

func stubbed(result *fetch.Result) *webFetch {
	return &webFetch{client: &stubGetter{result: result}}
}

func okResult() *fetch.Result {
	return &fetch.Result{
		URL:         "https://example.com",
		Status:      200,
		ContentType: "text/plain",
		Body:        []byte("ok"),
	}
}

func attribute(t *testing.T, c *pb.ToolContent, key string) string {
	t.Helper()
	for _, attr := range c.GetAttributes() {
		if attr.GetKey() == key {
			return attr.GetValue()
		}
	}
	return ""
}

func TestFacetSchemaIsValidJSONSchema(t *testing.T) {
	facet := newWebFetch().facet()

	var schema struct {
		Type       string                     `json:"type"`
		Properties map[string]json.RawMessage `json:"properties"`
		Required   []string                   `json:"required"`
	}
	if err := json.Unmarshal([]byte(facet.GetParameters()), &schema); err != nil {
		t.Fatalf("parameters are not valid json: %v", err)
	}

	if schema.Type != "object" {
		t.Errorf("schema type = %q, want object", schema.Type)
	}
	for _, property := range []string{"url", "format", "timeout_seconds"} {
		if _, ok := schema.Properties[property]; !ok {
			t.Errorf("schema is missing property %q", property)
		}
	}
	if len(schema.Required) != 1 || schema.Required[0] != "url" {
		t.Errorf("required = %v, want [url]", schema.Required)
	}
	if facet.GetDescription() == "" {
		t.Error("facet has no description")
	}
}

func TestInvokeRejectsBadArguments(t *testing.T) {
	cases := []struct{ name, args, want string }{
		{"malformed json", "not json", "invalid arguments"},
		{"unknown key", `{"url":"https://example.com","timeout":90}`, "unknown field"},
		{"unknown format", `{"url":"https://example.com","format":"pdf"}`, "unsupported format"},
		{"non-http scheme", `{"url":"file:///etc/passwd"}`, "scheme"},
		{"loopback target", `{"url":"http://127.0.0.1:1/"}`, "private"},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			_, err := newWebFetch().invoke(context.Background(), "call", c.args)

			if err == nil || !strings.Contains(err.Error(), c.want) {
				t.Fatalf("err = %v, want %q", err, c.want)
			}
		})
	}
}

func TestInvokeAcceptsFloatFormattedTimeout(t *testing.T) {
	// float-formatted integers are a common LLM output for integer-typed
	// schema fields
	_, err := stubbed(okResult()).invoke(context.Background(), "call", `{"url":"https://example.com","timeout_seconds":30.0}`)

	if err != nil {
		t.Fatalf("float-formatted timeout rejected a valid call: %v", err)
	}
}

func TestOverflowingTimeoutClampsToMax(t *testing.T) {
	stub := &stubGetter{result: okResult()}
	tool := &webFetch{client: stub}

	// schema bounds are advisory to the model; seconds this large overflow
	// int64 nanoseconds if converted before clamping
	_, err := tool.invoke(context.Background(), "call", `{"url":"https://example.com","timeout_seconds":18446744074}`)
	if err != nil {
		t.Fatalf("invoke: %v", err)
	}

	if stub.timeout != fetch.MaxTimeout {
		t.Errorf("timeout = %v, want the %v cap", stub.timeout, fetch.MaxTimeout)
	}
}

func TestInvokeRendersHTMLAsMarkdown(t *testing.T) {
	tool := stubbed(&fetch.Result{
		URL:         "https://example.com/page",
		Status:      200,
		ContentType: "text/html; charset=utf-8",
		Body:        []byte("<h1>Title</h1><p>Body text.</p>"),
	})

	got, err := tool.invoke(context.Background(), "call-1", `{"url":"https://example.com/page"}`)
	if err != nil {
		t.Fatalf("invoke: %v", err)
	}

	if !strings.Contains(got.GetText(), "# Title") {
		t.Errorf("text = %q, want markdown", got.GetText())
	}
	if attribute(t, got, "content_type") != "text/html" {
		t.Errorf("content_type = %q, want text/html", attribute(t, got, "content_type"))
	}
	if attribute(t, got, "format") != "markdown" {
		t.Errorf("format = %q, want markdown", attribute(t, got, "format"))
	}
}

func TestFormatAttributeReportsActualRendering(t *testing.T) {
	tool := stubbed(&fetch.Result{
		URL:         "https://api.example.com/data",
		Status:      200,
		ContentType: "application/json",
		Body:        []byte(`{"k":1}`),
	})

	// requested markdown (the default), but a JSON body passes through as
	// plain text — the attribute must say what the model actually received
	got, err := tool.invoke(context.Background(), "call", `{"url":"https://api.example.com/data"}`)
	if err != nil {
		t.Fatalf("invoke: %v", err)
	}

	if format := attribute(t, got, "format"); format != "text" {
		t.Errorf("format = %q, want text for a passthrough body", format)
	}
}

func TestInvokeRefusesBinaryContent(t *testing.T) {
	tool := stubbed(&fetch.Result{
		URL:         "https://example.com/logo.png",
		Status:      200,
		ContentType: "image/png",
		Body:        append([]byte("\x89PNG\r\n\x1a\n"), make([]byte, 32)...),
	})

	_, err := tool.invoke(context.Background(), "call-2", `{"url":"https://example.com/logo.png"}`)

	if err == nil || !strings.Contains(err.Error(), "image/png") {
		t.Fatalf("err = %v, want a binary refusal naming the media type", err)
	}
}

func TestBinaryRefusalReportsTruncatedSizeAsLowerBound(t *testing.T) {
	tool := stubbed(&fetch.Result{
		URL:         "https://example.com/big.pdf",
		Status:      200,
		ContentType: "application/pdf",
		Body:        make([]byte, 1000),
		Truncated:   true,
	})

	_, err := tool.invoke(context.Background(), "call", `{"url":"https://example.com/big.pdf"}`)

	if err == nil || !strings.Contains(err.Error(), "at least 1000 bytes") {
		t.Fatalf("err = %v, want the truncated read reported as a lower bound", err)
	}
}

func TestPoisonedURLSurvivesTheWire(t *testing.T) {
	result := &fetch.Result{
		// a redirect Location can carry raw legacy bytes; url.String()
		// emits the query verbatim
		URL:    "http://host/page?q=caf\xe9",
		Status: 200,
	}
	got := buildContent("call", result, content.Rendered{Text: "ok", MediaType: "text/html", Format: content.Markdown})

	// the transport writer owns the wire's UTF-8 guarantee; the frame must
	// still reach the host rather than dying in the marshal
	var pipe bytes.Buffer
	if err := transport.NewWriter(&pipe).Write(got); err != nil {
		t.Fatalf("write: %v", err)
	}
	decoded := &pb.ToolContent{}
	if err := transport.NewReader(&pipe).Read(decoded); err != nil {
		t.Fatalf("read back: %v", err)
	}

	if !utf8.ValidString(attribute(t, decoded, "url")) {
		t.Errorf("url attribute is not valid utf-8: %q", attribute(t, decoded, "url"))
	}
}

func TestBuildContentShape(t *testing.T) {
	result := &fetch.Result{
		URL:         "https://example.com/page",
		Status:      200,
		ContentType: "text/html; charset=utf-8",
	}
	rendered := content.Rendered{Text: "# Title", MediaType: "text/html", Format: content.Markdown}

	got := buildContent("call-1", result, rendered)

	if got.GetTag() != "web_fetch" {
		t.Errorf("tag = %q, want web_fetch", got.GetTag())
	}
	if got.GetText() != "# Title" {
		t.Errorf("text = %q", got.GetText())
	}
	want := map[string]string{
		"url":          "https://example.com/page",
		"status":       "200",
		"content_type": "text/html",
		"format":       "markdown",
		"fetch_id":     "call-1",
	}
	for key, value := range want {
		if got := attribute(t, got, key); got != value {
			t.Errorf("attribute %s = %q, want %q", key, got, value)
		}
	}
	if attribute(t, got, "fetch_truncated") != "" {
		t.Error("untruncated result should not carry a truncated attribute")
	}
}

func TestBuildContentFlagsTruncation(t *testing.T) {
	result := &fetch.Result{
		URL:       "https://example.com",
		Status:    200,
		Body:      []byte(strings.Repeat("a", 1234)),
		Truncated: true,
	}

	got := buildContent("call-2", result, content.Rendered{Text: "body", MediaType: "text/plain", Format: content.Text})

	if attribute(t, got, "fetch_truncated") != "true" {
		t.Error("truncated result missing its flag")
	}
	if attribute(t, got, "fetched_bytes") != "1234" {
		t.Errorf("fetched_bytes = %q, want the actual body size 1234", attribute(t, got, "fetched_bytes"))
	}
	// the host's spill augmentation appends its own truncated/total_bytes
	// attributes to this element; ours must not collide
	for _, reserved := range []string{"truncated", "truncated_at_bytes", "total_bytes", "full_output"} {
		if attribute(t, got, reserved) != "" {
			t.Errorf("attribute %q collides with the host's spill augmentation", reserved)
		}
	}
}
