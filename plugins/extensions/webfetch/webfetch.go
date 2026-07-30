package main

import (
	"context"
	"encoding/json"
	"fmt"
	"log"
	"strconv"
	"strings"
	"time"

	"paloma/extensions/webfetch/internal/content"
	"paloma/extensions/webfetch/internal/fetch"
	pb "paloma/extensions/webfetch/internal/pb/schema/extension"
)

const (
	extensionID  = "WebFetch"
	capabilityID = "WebFetch"
	description  = "Fetch a URL and return its content as markdown."
)

const toolDescription = `Fetch a web page over http(s) and return its content.

HTML is converted to markdown by default, keeping tables and strikethrough;
"text" extracts the page's plain text and "html" returns the source. Legacy
charsets are decoded to UTF-8. Only textual content is returned — images and
other binary types produce an error naming the media type. Requests to
private, loopback, and link-local addresses are refused, and oversized or
interrupted bodies are cut off with a truncated flag on the result.

Use this to read documentation, articles, or API responses the user references
by URL. It does not execute JavaScript, so pages that render entirely on the
client may come back close to empty.`

// parametersSchema is the JSON Schema the host hands to the model. The
// wording is written out so it stays reviewable; the timeout bounds are
// interpolated from the fetch package so they cannot drift from the policy
// actually enforced.
var parametersSchema = fmt.Sprintf(`{
  "type": "object",
  "properties": {
    "url": {
      "type": "string",
      "description": "Absolute http(s) URL to fetch, e.g. \"https://example.com/docs\"."
    },
    "format": {
      "type": "string",
      "enum": ["markdown", "text", "html"],
      "default": "markdown",
      "description": "markdown (default) converts HTML to markdown; text extracts the page's plain text; html returns the source unchanged."
    },
    "timeout_seconds": {
      "type": "integer",
      "minimum": 1,
      "maximum": %[1]d,
      "default": %[2]d,
      "description": "How long to wait for the response. Capped at %[1]d."
    }
  },
  "required": ["url"],
  "additionalProperties": false
}`, int(fetch.MaxTimeout.Seconds()), int(fetch.DefaultTimeout.Seconds()))

type arguments struct {
	URL    string `json:"url"`
	Format string `json:"format"`
	// float64, not int: models routinely emit 30.0 for integer-typed schema
	// fields, and rejecting the whole call over the formatting helps nobody
	TimeoutSeconds float64 `json:"timeout_seconds"`
}

// getter lets tests stub the fetch client so render paths run without the
// network.
type getter interface {
	Get(ctx context.Context, rawURL string, timeout time.Duration) (*fetch.Result, error)
}

type webFetch struct {
	client getter
}

func newWebFetch() *webFetch {
	return &webFetch{client: fetch.NewClient()}
}

func (w *webFetch) facet() *pb.ToolFacet {
	return &pb.ToolFacet{
		Description: toolDescription,
		Parameters:  parametersSchema,
	}
}

func (w *webFetch) invoke(ctx context.Context, callID, rawArguments string) (*pb.ToolContent, error) {
	// the schema declares additionalProperties: false and nothing upstream
	// validates, so this decode is the only enforcement point; a misnamed
	// key must fail loudly rather than silently fall back to defaults
	decoder := json.NewDecoder(strings.NewReader(rawArguments))
	decoder.DisallowUnknownFields()
	var args arguments
	if err := decoder.Decode(&args); err != nil {
		return nil, fmt.Errorf("invalid arguments: %w", err)
	}

	format, ok := content.ParseFormat(args.Format)
	if !ok {
		return nil, fmt.Errorf("unsupported format %q: expected markdown, text, or html", args.Format)
	}

	result, err := w.client.Get(ctx, args.URL, requestTimeout(args.TimeoutSeconds))
	if err != nil {
		return nil, err
	}
	if result.ReadErr != nil {
		log.Printf("fetching %s: body ended early: %v", result.URL, result.ReadErr)
	}

	rendered, err := content.Render(result.Body, result.ContentType, format)
	if err != nil {
		size := fmt.Sprintf("%d bytes", len(result.Body))
		if result.Truncated {
			// the read stopped at the cap; the resource itself is bigger
			size = "at least " + size
		}
		return nil, fmt.Errorf(
			"%s serves %s (%s), which has no text rendering; only textual content types are supported",
			result.URL, rendered.MediaType, size,
		)
	}
	if rendered.Degraded != nil {
		log.Printf("rendering %s: %v", result.URL, rendered.Degraded)
	}

	return buildContent(callID, result, rendered), nil
}

// requestTimeout owns only the float→Duration conversion; capping at
// fetch.MaxTimeout doubles as the int64-overflow guard, and non-positive
// values pass through for the fetch layer to apply its default.
func requestTimeout(seconds float64) time.Duration {
	return time.Duration(min(seconds, fetch.MaxTimeout.Seconds()) * float64(time.Second))
}

func buildContent(callID string, result *fetch.Result, rendered content.Rendered) *pb.ToolContent {
	attributes := make([]*pb.Attribute, 0, 7)
	attributes = append(attributes,
		&pb.Attribute{Key: "url", Value: result.URL},
		&pb.Attribute{Key: "status", Value: strconv.Itoa(result.Status)},
		&pb.Attribute{Key: "content_type", Value: rendered.MediaType},
		&pb.Attribute{Key: "format", Value: string(rendered.Format)},
		&pb.Attribute{Key: "fetch_id", Value: callID},
	)
	if result.Truncated {
		// fetch_-prefixed so they cannot collide with the truncated and
		// total_bytes attributes the host's spill augmentation appends to
		// this same element
		attributes = append(attributes,
			&pb.Attribute{Key: "fetch_truncated", Value: "true"},
			&pb.Attribute{Key: "fetched_bytes", Value: strconv.Itoa(len(result.Body))},
		)
	}

	return &pb.ToolContent{
		Tag:        "web_fetch",
		Attributes: attributes,
		Body:       &pb.ToolContent_Text{Text: rendered.Text},
	}
}
