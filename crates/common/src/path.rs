const FORBIDDEN: [char; 9] = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

pub fn sanitize_component(value: &str, max: usize, fallback: &str) -> String {
    let cleaned: String = value
        .chars()
        .map(|c| {
            if c.is_control() || FORBIDDEN.contains(&c) {
                '_'
            } else {
                c
            }
        })
        .collect();

    let trimmed = cleaned.trim().trim_matches('.');
    let truncated: String = trimmed.chars().take(max).collect();
    let truncated = truncated.trim_end();

    if truncated.is_empty() {
        fallback.to_owned()
    } else {
        truncated.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::sanitize_component;

    #[test]
    fn replaces_path_separators() {
        assert_eq!(sanitize_component("AC/DC", 64, "x"), "AC_DC");
        assert_eq!(sanitize_component("a:b*c?", 64, "x"), "a_b_c_");
    }

    #[test]
    fn replaces_control_characters() {
        assert_eq!(sanitize_component("a\nb", 64, "x"), "a_b");
    }

    #[test]
    fn trims_whitespace_and_dots() {
        assert_eq!(sanitize_component("  .hidden.  ", 64, "x"), "hidden");
    }

    #[test]
    fn truncates_to_max() {
        assert_eq!(sanitize_component("abcdefgh", 3, "x"), "abc");
    }

    #[test]
    fn trims_trailing_space_left_by_truncation() {
        assert_eq!(sanitize_component("ab cdef", 3, "x"), "ab");
    }

    #[test]
    fn falls_back_when_nothing_remains() {
        assert_eq!(sanitize_component("", 64, "untitled"), "untitled");
        assert_eq!(sanitize_component("...", 64, "untitled"), "untitled");
    }

    #[test]
    fn counts_characters_not_bytes() {
        assert_eq!(sanitize_component("ééééé", 3, "x"), "ééé");
    }
}
