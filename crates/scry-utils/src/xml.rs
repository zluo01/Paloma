use std::fmt::{self, Display, Formatter, Write};

#[derive(Debug, Clone)]
pub struct Element {
    name: &'static str,
    attrs: Vec<(&'static str, String)>,
    body: Body,
}

#[derive(Debug, Clone)]
enum Body {
    Empty,
    PlainText(String),
    Cdata(String),
    Children(Vec<Element>),
}

impl Element {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            attrs: Vec::new(),
            body: Body::Empty,
        }
    }

    pub fn attr(mut self, key: &'static str, value: impl Display) -> Self {
        self.attrs.push((key, escape_attr(&value.to_string())));
        self
    }

    pub fn attr_if(self, cond: bool, key: &'static str, value: impl Display) -> Self {
        if cond {
            self.attr(key, value)
        } else {
            self
        }
    }

    pub fn attr_if_some<T: Display>(self, key: &'static str, value: Option<T>) -> Self {
        match value {
            Some(v) => self.attr(key, v),
            None => self,
        }
    }

    pub fn cdata(mut self, text: impl Into<String>) -> Self {
        let raw = text.into();
        self.body = Body::Cdata(escape_cdata(&raw));
        self
    }

    pub fn plain_text(mut self, text: impl Into<String>) -> Self {
        self.body = Body::PlainText(text.into());
        self
    }

    pub fn child(mut self, child: Element) -> Self {
        match &mut self.body {
            Body::Children(v) => v.push(child),
            _ => self.body = Body::Children(vec![child]),
        }
        self
    }

    pub fn children<I>(mut self, children: I) -> Self
    where
        I: IntoIterator<Item = Element>,
    {
        let iter = children.into_iter();
        match &mut self.body {
            Body::Children(v) => v.extend(iter),
            _ => self.body = Body::Children(iter.collect()),
        }
        self
    }
}

impl Display for Element {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write_element(f, self)
    }
}

fn write_element(f: &mut Formatter<'_>, elem: &Element) -> fmt::Result {
    write!(f, "<{}", elem.name)?;
    for (k, v) in &elem.attrs {
        // v is already attr-escaped at construction time.
        write!(f, "\n  {k}=\"{v}\"")?;
    }

    let has_attrs = !elem.attrs.is_empty();

    match &elem.body {
        Body::Empty => {
            if has_attrs {
                write!(f, "\n></{}>", elem.name)
            } else {
                write!(f, "></{}>", elem.name)
            }
        },
        Body::PlainText(body) => {
            if has_attrs {
                write!(f, "\n>{body}</{}>", elem.name)
            } else {
                write!(f, ">{body}</{}>", elem.name)
            }
        },
        Body::Cdata(escaped) => {
            // Body already has its `]]>` occurrences split.
            if has_attrs {
                write!(f, "\n><![CDATA[{escaped}]]></{}>", elem.name)
            } else {
                write!(f, "><![CDATA[{escaped}]]></{}>", elem.name)
            }
        },
        Body::Children(kids) => {
            if has_attrs {
                f.write_str("\n>")?;
            } else {
                f.write_char('>')?;
            }
            for kid in kids {
                f.write_char('\n')?;
                write_element(f, kid)?;
            }
            write!(f, "\n</{}>", elem.name)
        },
    }
}

fn escape_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '"' => out.push_str("&quot;"),
            other => out.push(other),
        }
    }
    out
}

fn escape_cdata(s: &str) -> String {
    s.replace("]]>", "]]]]><![CDATA[>")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_element_renders_as_self_pair() {
        let actual = Element::new("foo").to_string();
        let expected = "<foo></foo>";
        assert_eq!(actual, expected);
    }

    #[test]
    fn attrs_render_one_per_line() {
        let actual = Element::new("foo")
            .attr("bar", "baz")
            .attr("n", 42)
            .to_string();
        let expected = "<foo\n  bar=\"baz\"\n  n=\"42\"\n></foo>";
        assert_eq!(actual, expected);
    }

    #[test]
    fn attr_value_escapes_ampersand_lt_and_quote() {
        let actual = Element::new("foo").attr("v", r#"a"b<c&d"#).to_string();
        let expected = "<foo\n  v=\"a&quot;b&lt;c&amp;d\"\n></foo>";
        assert_eq!(actual, expected);
    }

    #[test]
    fn attr_value_leaves_gt_and_apostrophe_alone() {
        // These are fine inside double-quoted attribute values.
        let actual = Element::new("foo").attr("v", "a>b'c").to_string();
        let expected = "<foo\n  v=\"a>b'c\"\n></foo>";
        assert_eq!(actual, expected);
    }

    #[test]
    fn attr_if_includes_when_true_drops_when_false() {
        let actual = Element::new("foo")
            .attr_if(true, "a", 1)
            .attr_if(false, "b", 2)
            .to_string();
        let expected = "<foo\n  a=\"1\"\n></foo>";
        assert_eq!(actual, expected);
    }

    #[test]
    fn attr_if_some_includes_when_some_drops_when_none() {
        let actual = Element::new("foo")
            .attr_if_some("a", Some(1))
            .attr_if_some::<i32>("b", None)
            .to_string();
        let expected = "<foo\n  a=\"1\"\n></foo>";
        assert_eq!(actual, expected);
    }

    #[test]
    fn cdata_body_is_wrapped_verbatim() {
        let actual = Element::new("stdout").cdata("hello\nworld").to_string();
        let expected = "<stdout><![CDATA[hello\nworld]]></stdout>";
        assert_eq!(actual, expected);
    }

    #[test]
    fn plain_text_body_renders_inline() {
        let actual = Element::new("cwd").plain_text("/home/mike").to_string();
        let expected = "<cwd>/home/mike</cwd>";
        assert_eq!(actual, expected);
    }

    #[test]
    fn plain_text_passes_through_verbatim_without_escaping() {
        // By contract, plain_text is unescaped — the caller guarantees the
        // value is free of XML metacharacters. Anything that would need
        // escaping should use `cdata` instead.
        let actual = Element::new("v").plain_text("a & b < c > d").to_string();
        let expected = "<v>a & b < c > d</v>";
        assert_eq!(actual, expected);
    }

    #[test]
    fn plain_text_with_attrs_keeps_attrs_multiline_but_body_inline() {
        let actual = Element::new("p")
            .attr("k", 1)
            .plain_text("hello")
            .to_string();
        let expected = "<p\n  k=\"1\"\n>hello</p>";
        assert_eq!(actual, expected);
    }

    #[test]
    fn plain_text_replaces_cdata_body_when_set_after() {
        // Documenting current behaviour: last body wins, same as cdata-then-child.
        let actual = Element::new("p")
            .cdata("dropped")
            .plain_text("kept")
            .to_string();
        let expected = "<p>kept</p>";
        assert_eq!(actual, expected);
    }

    #[test]
    fn cdata_with_attrs_keeps_attrs_multiline_but_body_inline() {
        let actual = Element::new("stdout")
            .attr("bytes", 11)
            .cdata("hello world")
            .to_string();
        let expected = "<stdout\n  bytes=\"11\"\n><![CDATA[hello world]]></stdout>";
        assert_eq!(actual, expected);
    }

    #[test]
    fn cdata_splits_literal_terminator_across_sections() {
        // The literal sequence `]]>` inside CDATA must be split. After the
        // split, the two CDATA bodies concatenated equal the original.
        let actual = Element::new("x").cdata("foo]]>bar").to_string();
        let expected = "<x><![CDATA[foo]]]]><![CDATA[>bar]]></x>";
        assert_eq!(actual, expected);
    }

    #[test]
    fn cdata_passes_through_arbitrary_bytes() {
        // Quotes, backslashes, angle brackets, ampersands — all literal,
        // because CDATA doesn't interpret them.
        let input = "path\\to\\file with \"quotes\" & <tags>";
        let actual = Element::new("x").cdata(input).to_string();
        assert!(actual.contains(input), "got: {actual}");
    }

    #[test]
    fn nested_children_render_with_newline_separators() {
        let actual = Element::new("parent")
            .child(Element::new("a").cdata("1"))
            .child(Element::new("b").cdata("2"))
            .to_string();
        let expected = "<parent>\n<a><![CDATA[1]]></a>\n<b><![CDATA[2]]></b>\n</parent>";
        assert_eq!(actual, expected);
    }

    #[test]
    fn child_after_cdata_replaces_the_body() {
        // Documenting current behaviour: `.cdata` then `.child` drops the
        // CDATA. The intended usage is "pick one body shape per element".
        let actual = Element::new("p")
            .cdata("dropped")
            .child(Element::new("c"))
            .to_string();
        let expected = "<p>\n<c></c>\n</p>";
        assert_eq!(actual, expected);
    }

    #[test]
    fn children_iter_appends_in_order() {
        let kids = (0..3).map(|i| Element::new("k").attr("i", i));
        let actual = Element::new("parent").children(kids).to_string();
        assert!(actual.starts_with("<parent>\n<k"));
        assert!(actual.contains("i=\"0\""));
        assert!(actual.contains("i=\"1\""));
        assert!(actual.contains("i=\"2\""));
        assert!(actual.ends_with("</parent>"));
    }

    #[test]
    fn shell_output_realistic_shape() {
        // Sanity check for the actual planned shell payload shape.
        let actual = Element::new("shell_output")
            .attr("command", "ls -la")
            .attr("workdir", "/home/mike")
            .attr("exit_code", 0)
            .attr("duration_ms", 12)
            .child(
                Element::new("stdout")
                    .attr("bytes", 5)
                    .attr("total_bytes", 5)
                    .attr("truncated", false)
                    .cdata("file1"),
            )
            .child(
                Element::new("stderr")
                    .attr("bytes", 0)
                    .attr("total_bytes", 0)
                    .attr("truncated", false),
            )
            .to_string();

        assert!(actual.starts_with("<shell_output"));
        assert!(actual.contains("command=\"ls -la\""));
        assert!(actual.contains("workdir=\"/home/mike\""));
        assert!(actual.contains("<![CDATA[file1]]>"));
        assert!(actual.contains("<stderr"));
        assert!(actual.ends_with("</shell_output>"));
    }

    #[test]
    fn display_streams_without_extra_allocation_for_writeln() {
        // Smoke test that we can write directly to a String via `write!`
        // without going through `to_string`.
        let mut out = String::new();
        let elem = Element::new("x").attr("k", 1).cdata("body");
        write!(&mut out, "{elem}").unwrap();
        assert_eq!(out, "<x\n  k=\"1\"\n><![CDATA[body]]></x>");
    }
}
