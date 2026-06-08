use std::fmt::Write;

use sha1::{Digest, Sha1};

const PREFIX: &str = "mcp__";
const DELIM: &str = "__";

const MAX_TOOL_NAME_LENGTH: usize = 64;
const HASH_LEN: usize = 12;

/// generate unique hash for original mcp tool name.
fn hash_suffix(identity: &str) -> String {
    let digest = Sha1::digest(identity.as_bytes());
    let mut out = String::with_capacity(HASH_LEN + 1);
    out.push('_');
    for byte in digest.iter().take(HASH_LEN / 2) {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// truncate string on bytes instead of characters
fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

pub fn mcp_function_name_encode(server_name: &str, tool_name: &str) -> String {
    let namespace = format!("{PREFIX}{server_name}{DELIM}");
    let full = format!("{namespace}{tool_name}");
    if full.len() <= MAX_TOOL_NAME_LENGTH {
        return full;
    }

    // Hash the untruncated identity so the suffix is stable and collision-resistant.
    let suffix = hash_suffix(&full);
    let room_for_tool = MAX_TOOL_NAME_LENGTH.saturating_sub(namespace.len());
    if room_for_tool >= suffix.len() {
        // Keep the namespace; truncate the tool to make room for the suffix.
        let keep = room_for_tool - suffix.len();
        format!("{namespace}{}{suffix}", truncate(tool_name, keep))
    } else {
        // Namespace alone already overflows: truncate it and drop the tool,
        // keeping only the hash so distinct tools stay distinct.
        let keep = MAX_TOOL_NAME_LENGTH.saturating_sub(suffix.len());
        format!("{}{suffix}", truncate(&namespace, keep))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_simple_names() {
        let name = mcp_function_name_encode("spotify", "search");
        assert_eq!(name, "mcp__spotify__search");
    }

    #[test]
    fn segments_are_passed_through_verbatim() {
        assert_eq!(
            mcp_function_name_encode("spotify", "get__track"),
            "mcp__spotify__get__track"
        );
        assert_eq!(
            mcp_function_name_encode("my__server", "search"),
            "mcp__my__server__search"
        );
    }

    #[test]
    fn name_at_the_limit_is_left_untouched() {
        // namespace "mcp__s__" is 8 bytes, so a 56-byte tool hits exactly 64.
        let tool = "t".repeat(MAX_TOOL_NAME_LENGTH - "mcp__s__".len());
        let name = mcp_function_name_encode("s", &tool);
        assert_eq!(name.len(), MAX_TOOL_NAME_LENGTH);
        assert_eq!(name, format!("mcp__s__{tool}"));
    }

    #[test]
    fn over_long_name_is_truncated_with_hash_suffix() {
        let tool = "a".repeat(200);
        let name = mcp_function_name_encode("spotify", &tool);

        assert_eq!(name.len(), MAX_TOOL_NAME_LENGTH);
        // Namespace is preserved, tool is truncated.
        assert!(name.starts_with("mcp__spotify__"));
        // Ends with `_<12 hex>`.
        let suffix = &name[name.len() - (HASH_LEN + 1)..];
        let mut chars = suffix.chars();
        assert_eq!(chars.next(), Some('_'));
        assert!(chars.all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn truncation_is_deterministic() {
        let tool = "a".repeat(200);
        let first = mcp_function_name_encode("spotify", &tool);
        let second = mcp_function_name_encode("spotify", &tool);
        assert_eq!(first, second);
    }

    #[test]
    fn distinct_tools_sharing_a_prefix_get_distinct_names() {
        // Same 100-char prefix, differing only past the truncation point. The
        // hash of the full identity keeps the encoded names apart.
        let a = format!("{}_alpha", "x".repeat(100));
        let b = format!("{}_beta", "x".repeat(100));
        let name_a = mcp_function_name_encode("srv", &a);
        let name_b = mcp_function_name_encode("srv", &b);
        assert_ne!(name_a, name_b);
        assert_eq!(name_a.len(), MAX_TOOL_NAME_LENGTH);
        assert_eq!(name_b.len(), MAX_TOOL_NAME_LENGTH);
    }

    #[test]
    fn over_long_server_truncates_namespace_and_drops_tool() {
        // Namespace alone (mcp__ + 100 chars + __) already exceeds the limit, so
        // the tool drops out entirely and only the hash keeps it unique.
        let server = "s".repeat(100);
        let name = mcp_function_name_encode(&server, "search");
        assert_eq!(name.len(), MAX_TOOL_NAME_LENGTH);
        assert!(name.starts_with("mcp__"));
        let suffix = &name[name.len() - (HASH_LEN + 1)..];
        assert!(suffix.starts_with('_'));
        assert!(suffix[1..].chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn truncation_never_splits_a_utf8_char() {
        // A multibyte tool name long enough to force truncation must still
        // produce valid UTF-8 within the byte limit (no panic, no broken char).
        let tool = "é".repeat(100); // 2 bytes each
        let name = mcp_function_name_encode("srv", &tool);
        assert!(name.len() <= MAX_TOOL_NAME_LENGTH);
        // Round-trips through str means it is valid UTF-8 by construction; assert
        // the boundary explicitly for clarity.
        assert!(name.is_char_boundary(name.len()));
    }
}
