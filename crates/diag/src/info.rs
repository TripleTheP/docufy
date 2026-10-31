//! The system information for a bug report ([`report`]), without personal data: the user's home
//! folder and name are replaced wherever they appear ([`redact`]).

/// `lines` as "Label: value" lines, one per line, personal paths and the user name replaced.
/// `home`: the user's home folder; `user`: their login name.
pub fn report(lines: &[(String, String)], home: Option<&str>, user: Option<&str>) -> String {
    let text: String = lines.iter().map(|(label, value)| format!("{label}: {value}\n")).collect();
    redact(&text, home, user)
}

/// `text` with the home folder written as `~` and the user name as `<user>`, in any letter case
/// and with either slash. Names shorter than three letters are left alone: they would match
/// inside ordinary words.
pub fn redact(text: &str, home: Option<&str>, user: Option<&str>) -> String {
    let mut out = text.to_string();
    if let Some(home) = home.map(|h| h.trim_end_matches(['/', '\\'])).filter(|h| h.len() > 3) {
        for form in [home.to_string(), home.replace('\\', "/"), home.replace('/', "\\")] {
            out = replace_ignoring_case(&out, &form, "~");
        }
    }
    if let Some(user) = user.map(str::trim).filter(|u| u.chars().count() >= 3) {
        out = replace_ignoring_case(&out, user, "<user>");
    }
    out
}

fn replace_ignoring_case(text: &str, what: &str, with: &str) -> String {
    let lower = text.to_lowercase();
    let what_lower = what.to_lowercase();
    // Lowercasing can change byte lengths (e.g. 'İ'); then only exact matches are replaced.
    if lower.len() != text.len() || what_lower.len() != what.len() || what.is_empty() {
        return text.replace(what, with);
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = 0;
    for (at, _) in lower.match_indices(&what_lower) {
        if at < rest {
            continue;
        }
        out.push_str(&text[rest..at]);
        out.push_str(with);
        rest = at + what.len();
    }
    out.push_str(&text[rest..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_folders_and_user_names_are_left_out() {
        let lines = vec![
            ("Log file".to_string(), r"C:\Users\Lovelace\AppData\Roaming\Docufy\logs\docufy.log".to_string()),
            ("Settings".to_string(), "c:/users/lovelace/AppData/Roaming/Docufy".to_string()),
            ("Computer".to_string(), "LOVELACE-PC".to_string()),
        ];
        let r = report(&lines, Some(r"C:\Users\Lovelace"), Some("Lovelace"));
        assert_eq!(r, "Log file: ~\\AppData\\Roaming\\Docufy\\logs\\docufy.log\nSettings: ~/AppData/Roaming/Docufy\nComputer: <user>-PC\n");
        assert!(!r.to_lowercase().contains("lovelace"));
    }

    #[test]
    fn short_names_and_missing_values_change_nothing() {
        assert_eq!(redact("Al was here", Some("/"), Some("Al")), "Al was here");
        assert_eq!(redact("nothing", None, None), "nothing");
        assert_eq!(redact("/home/ada/x", Some("/home/ada/"), None), "~/x");
    }
}
