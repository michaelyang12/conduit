use crate::error::{Error, Result};
use serde::de::DeserializeOwned;
use serde_json::Value;

const INSTRUCTION: &str =
    "Respond with a single JSON value and nothing else: no prose, no markdown, no code fences.";

pub(crate) fn instruction(schema: Option<&Value>) -> String {
    match schema {
        Some(schema) => format!("{INSTRUCTION}\nThe JSON must match this JSON Schema:\n{schema}"),
        None => INSTRUCTION.to_string(),
    }
}

/// Parses model output as JSON, tolerating the wrappers models add even when told not to.
pub(crate) fn parse<T: DeserializeOwned>(raw: &str) -> Result<T> {
    let unfenced = strip_fence(raw.trim());
    let first_err = match serde_json::from_str(unfenced) {
        Ok(v) => return Ok(v),
        Err(e) => e,
    };
    if let Some(inner) = outermost_value(unfenced)
        && let Ok(v) = serde_json::from_str(inner)
    {
        return Ok(v);
    }
    Err(Error::Json {
        raw: raw.to_string(),
        source: first_err,
    })
}

fn strip_fence(text: &str) -> &str {
    let Some(rest) = text.strip_prefix("```") else {
        return text;
    };
    let body = rest.split_once('\n').map_or("", |(_, body)| body);
    body.trim_end().strip_suffix("```").unwrap_or(body).trim()
}

fn outermost_value(text: &str) -> Option<&str> {
    let start = text.find(['{', '['])?;
    let close = if text.as_bytes()[start] == b'{' {
        '}'
    } else {
        ']'
    };
    let end = text.rfind(close)?;
    (end > start).then(|| &text[start..=end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Fix {
        cause: String,
    }

    fn fix(raw: &str) -> Fix {
        parse(raw).unwrap()
    }

    #[test]
    fn parses_plain_fenced_and_chatty_output() {
        let want = Fix { cause: "x".into() };
        assert_eq!(fix(r#"{"cause":"x"}"#), want);
        assert_eq!(fix("```json\n{\"cause\":\"x\"}\n```"), want);
        assert_eq!(fix("```\n{\"cause\":\"x\"}\n```"), want);
        assert_eq!(
            fix("Sure! Here it is:\n{\"cause\":\"x\"}\nHope that helps."),
            want
        );
    }

    #[test]
    fn parses_arrays() {
        let v: Vec<u8> = parse("Result: [1, 2, 3]").unwrap();
        assert_eq!(v, vec![1, 2, 3]);
    }

    #[test]
    fn keeps_raw_text_on_failure() {
        match parse::<Fix>("not json at all") {
            Err(Error::Json { raw, .. }) => assert_eq!(raw, "not json at all"),
            other => panic!("expected Json error, got {other:?}"),
        }
    }
}
