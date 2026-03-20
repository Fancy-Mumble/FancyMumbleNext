//! HTML utility functions.

/// Strip HTML tags from a string, returning only text content.
///
/// This is a crude `<`/`>` based stripper that does not handle edge
/// cases like tags inside attribute values. Good enough for display
/// and notification purposes.
pub fn strip_html_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut inside_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => inside_tag = true,
            '>' => inside_tag = false,
            _ if !inside_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_simple_tags() {
        assert_eq!(strip_html_tags("<b>hello</b>"), "hello");
    }

    #[test]
    fn strip_nested_tags() {
        assert_eq!(
            strip_html_tags("<div><p>text</p></div>"),
            "text"
        );
    }

    #[test]
    fn strip_preserves_plain_text() {
        assert_eq!(strip_html_tags("no tags here"), "no tags here");
    }

    #[test]
    fn strip_empty() {
        assert_eq!(strip_html_tags(""), "");
    }

    #[test]
    fn strip_with_attributes() {
        assert_eq!(
            strip_html_tags(r#"<a href="http://example.com">link</a>"#),
            "link"
        );
    }
}
