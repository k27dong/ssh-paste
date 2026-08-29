pub fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

pub fn remote_path_expr(path: &str) -> String {
    if path == "~" {
        return "\"$HOME\"".into();
    }
    match path.strip_prefix("~/") {
        Some(rest) => format!("\"$HOME\"/{}", sh_quote(rest)),
        None => sh_quote(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_plain_and_hostile_strings() {
        assert_eq!(sh_quote("abc"), "'abc'");
        assert_eq!(sh_quote("a'b"), r"'a'\''b'");
        assert_eq!(sh_quote("$HOME `x` \"y\""), "'$HOME `x` \"y\"'");
    }

    #[test]
    fn expands_home_prefix_only() {
        assert_eq!(
            remote_path_expr("~/.cache/ssh-paste"),
            "\"$HOME\"/'.cache/ssh-paste'"
        );
        assert_eq!(remote_path_expr("~"), "\"$HOME\"");
        assert_eq!(remote_path_expr("/var/tmp/x"), "'/var/tmp/x'");
    }
}
