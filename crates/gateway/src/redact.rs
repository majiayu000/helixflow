pub(crate) fn redact_sensitive(value: &str, api_key: &str) -> String {
    let redacted = if api_key.is_empty() {
        value.to_owned()
    } else {
        value.replace(api_key, "[redacted]")
    };
    redact_token_after_prefixes(&redacted, &["Bearer ", "bearer ", "Key ", "key "])
}

fn redact_token_after_prefixes(value: &str, prefixes: &[&str]) -> String {
    let mut output = String::with_capacity(value.len());
    let mut rest = value;
    while !rest.is_empty() {
        if let Some(prefix) = prefixes
            .iter()
            .copied()
            .find(|prefix| rest.starts_with(*prefix))
        {
            let prefix_len = prefix.len();
            output.push_str(prefix);
            output.push_str("[redacted]");
            let token_len = rest[prefix_len..]
                .find(|ch: char| ch.is_whitespace() || matches!(ch, '"' | '\'' | ',' | '}'))
                .unwrap_or(rest.len() - prefix_len);
            rest = &rest[prefix_len + token_len..];
        } else if let Some(ch) = rest.chars().next() {
            output.push(ch);
            rest = &rest[ch.len_utf8()..];
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::redact_sensitive;

    #[test]
    fn redacts_api_key_and_bearer_tokens() {
        assert_eq!(
            redact_sensitive("Bearer secret-key failed", "secret-key"),
            "Bearer [redacted] failed"
        );
        assert_eq!(
            redact_sensitive("using secret-key in body", "secret-key"),
            "using [redacted] in body"
        );
    }
}
