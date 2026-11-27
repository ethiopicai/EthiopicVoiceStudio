//! Last check before starting a synthesis worker. Spoken text is prepared in the
//! editor; reject raw numbers rather than sending them to the model silently.
use serde::Deserialize;
use std::{collections::BTreeMap, sync::OnceLock};

#[derive(Deserialize)]
struct Rules {
    version: String,
    fold: BTreeMap<char, char>,
}
fn rules() -> &'static Rules {
    static RULES: OnceLock<Rules> = OnceLock::new();
    RULES.get_or_init(|| {
        serde_json::from_str(include_str!("../../ui/src/speech-text-rules.json"))
            .expect("bundled speech text rules")
    })
}
pub fn version() -> &'static str {
    &rules().version
}
fn abbreviation_patterns() -> &'static Vec<regex::Regex> {
    static PATTERNS: OnceLock<Vec<regex::Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        #[derive(Deserialize)]
        struct Abbreviations {
            patterns: Vec<(String, String)>,
        }
        let data: Abbreviations =
            serde_json::from_str(include_str!("../../ui/src/speech-abbreviations.json"))
                .expect("bundled speech abbreviations");
        data.patterns
            .iter()
            .map(|(pattern, _)| regex::Regex::new(pattern).expect("bundled abbreviation pattern"))
            .collect()
    })
}
pub fn verify(text: &str, allow_empty: bool) -> Result<(), String> {
    if text.chars().count() > 12000 {
        return Err(
            "Use up to 12,000 characters after expanding numbers and abbreviations.".into(),
        );
    }
    if text.contains('/')
        || text.contains('.')
            && abbreviation_patterns()
                .iter()
                .any(|pattern| pattern.is_match(text))
    {
        return Err("Expand abbreviations and check the speech text before generating.".into());
    }
    if text.chars().any(|c| {
        c.is_numeric()
            || c.is_control()
            || matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{206f}' | '\u{feff}' | '$' | '€' | '£' | '¥' | '₹' | '%' | '٪')
    }) {
        return Err("Check the speech text and write numbers and numeric symbols in words before generating.".into());
    }
    let canonical: String = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .map(|c| rules().fold.get(&c).copied().unwrap_or(c))
        .collect();
    if canonical != text
        || (!allow_empty || !text.is_empty()) && !text.chars().any(char::is_alphabetic)
    {
        return Err("Check and normalize the speech text before generating.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refuse_unprepared_speech_before_model_work() {
        for text in [
            "12",
            "ሰላም ፲፪",
            "١٢",
            "１２",
            "½",
            "ሰላም\0",
            "ሐይል",
            " ሰላም",
            "ሰላም  ጤና",
            "ሰላም\u{200b}",
            "$ አስር",
            "!",
            "ዶ/ር አበበ",
            "ኢ//ር ሀና",
            "ዶ.ር አበበ",
            "ኢ.ር ሀና",
            "ት/ቤት",
        ] {
            assert!(verify(text, false).is_err(), "{text:?}");
        }
        assert!(verify("ሰላም አስራ ሁለት።", false).is_ok());
        assert!(verify("አስራ ሁለት ነጥብ ዜሮ አምስት", false).is_ok());
        assert!(verify("ዶክተር አበበ እና ኢንጂነር ሀና።", false).is_ok());
        assert!(verify("ዶክተር አበበ.", false).is_ok());
        assert!(verify("", true).is_ok());
        assert!(verify("", false).is_err());
        assert!(verify(&"ሀ".repeat(12001), false).is_err());
        assert_eq!(version(), "am-speech-v2");
    }
}
