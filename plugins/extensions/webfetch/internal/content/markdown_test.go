package content

import (
	"strings"
	"testing"
)

func TestRenderMarkdownRendersTables(t *testing.T) {
	html := `<table><thead><tr><th>Name</th><th>Age</th></tr></thead>
	<tbody><tr><td>Ada</td><td>36</td></tr><tr><td>Bob</td><td>41</td></tr></tbody></table>`

	got, err := Render([]byte(html), "text/html", Markdown)
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	for _, want := range []string{"| Name | Age |", "| Ada", "| Bob"} {
		if !strings.Contains(got.Text, want) {
			t.Errorf("markdown table missing %q, got:\n%s", want, got.Text)
		}
	}
	if strings.Contains(got.Text, "NameAge") {
		t.Errorf("table cells glued together:\n%s", got.Text)
	}
}

func TestRenderMarkdownRendersStrikethrough(t *testing.T) {
	html := `<p>Was <del>wrong</del> before.</p>`

	got, err := Render([]byte(html), "text/html", Markdown)
	if err != nil {
		t.Fatalf("render: %v", err)
	}

	if !strings.Contains(got.Text, "~~wrong~~") {
		t.Errorf("strikethrough lost, got:\n%s", got.Text)
	}
}
