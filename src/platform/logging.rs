//! Log-safe formatting helpers.

/// Remove Subsonic auth parameters (`t`, `s`, `p`) from a URL or a string that
/// embeds one, so tokens/salts never reach logs or user-facing errors.
pub fn redact_url(input: &str) -> String {
    let Some(start) = input.find("http://").or_else(|| input.find("https://")) else {
        return input.to_string();
    };
    let end = input[start..]
        .find(char::is_whitespace)
        .map(|offset| start + offset)
        .unwrap_or(input.len());
    let url_str = &input[start..end];
    let Ok(mut url) = url::Url::parse(url_str) else {
        return input.to_string();
    };
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| k != "t" && k != "s" && k != "p")
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    url.query_pairs_mut().clear().extend_pairs(kept);
    format!("{}{}{}", &input[..start], url, &input[end..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_auth_query_parameters() {
        let redacted = redact_url("https://x/rest/stream.view?id=1&u=a&t=secret&s=salt");
        assert!(redacted.contains("id=1"));
        assert!(!redacted.contains("secret"));
        assert!(!redacted.contains("salt"));
    }

    #[test]
    fn redacts_embedded_parameters_in_error_text() {
        let redacted = redact_url("failed to fetch https://x/rest?t=secret&s=salt: timeout");
        assert!(!redacted.contains("secret"));
        assert!(!redacted.contains("salt"));
        assert!(redacted.contains("timeout"));
    }
}
