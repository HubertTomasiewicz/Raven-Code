//! Literal and regular-expression search & replace.
//!
//! Byte ranges are returned so callers can drive caret movement and
//! selection in the UI layer without re-implementing the matching rules
//! (and so regex offsets are always byte-accurate).

use regex::RegexBuilder;

/// A search configuration. Literal search is implemented as a regex over the
/// escaped pattern so case handling and byte offsets stay correct in one
/// code path (Unicode lowercasing can change byte lengths; regexes cannot).
#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub text: String,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
}

impl SearchQuery {
    pub fn new(text: impl Into<String>) -> SearchQuery {
        SearchQuery {
            text: text.into(),
            case_sensitive: false,
            whole_word: false,
            regex: false,
        }
    }

    /// Builds the compiled matcher, mapping configuration problems to
    /// readable errors (invalid regex, empty pattern with word boundaries).
    pub fn compile(&self) -> Result<regex::Regex, String> {
        if self.text.is_empty() {
            return Err("empty search pattern".to_string());
        }
        let pattern = if self.regex {
            self.text.clone()
        } else {
            regex::escape(&self.text)
        };
        let wrapped = if self.whole_word {
            format!(r"\b(?:{pattern})\b")
        } else {
            pattern
        };
        RegexBuilder::new(&wrapped)
            .case_insensitive(!self.case_sensitive)
            .unicode(true)
            .size_limit(10 * 1024 * 1024)
            .build()
            .map_err(|error| error.to_string())
    }
}

/// A match as a half-open byte range into the searched text.
pub type MatchRange = std::ops::Range<usize>;

/// Finds all matches, capped at `limit` (protection against pathological
/// regexes flooding the UI with millions of results).
pub fn find_all(text: &str, query: &SearchQuery, limit: usize) -> Result<Vec<MatchRange>, String> {
    let re = query.compile()?;
    let mut out = Vec::new();
    for capture in re.find_iter(text) {
        out.push(capture.range());
        if out.len() >= limit {
            break;
        }
    }
    Ok(out)
}

/// Finds the next match at or after `from`, wrapping around to the start
/// when `wrap` is set. Returns the match containing/starting at `from` too,
/// which is what editors conventionally do on "find next" from a selection.
pub fn find_next(
    text: &str,
    query: &SearchQuery,
    from: usize,
    wrap: bool,
) -> Result<Option<MatchRange>, String> {
    let re = query.compile()?;
    let start = from.min(text.len());
    if let Some(found) = re.find_at(text, start) {
        return Ok(Some(found.range()));
    }
    if wrap && start > 0 {
        // Search the head region (matches before `from`), which cannot
        // overlap the tail search above except for one match spanning the
        // boundary — acceptable for find-next semantics.
        if let Some(found) = re.find(text) {
            return Ok(Some(found.range()));
        }
    }
    Ok(None)
}

pub fn find_previous(
    text: &str,
    query: &SearchQuery,
    before: usize,
    wrap: bool,
) -> Result<Option<MatchRange>, String> {
    let all = find_all(text, query, 1_000_000)?;
    let last_before = all
        .iter()
        .filter(|range| range.start < before)
        .max_by_key(|range| range.start);
    if let Some(found) = last_before {
        return Ok(Some(found.clone()));
    }
    if wrap {
        return Ok(all.first().cloned());
    }
    Ok(None)
}

/// Replaces every match with the replacement. When the query is a regex,
/// `$1`-style group references in `replacement` are honored by the regex
/// crate's `replace_all`; literal search replaces the raw text verbatim.
pub fn replace_all(
    text: &str,
    query: &SearchQuery,
    replacement: &str,
    limit: usize,
) -> Result<(String, usize), String> {
    let re = query.compile()?;
    let count = re.find_iter(text).take(limit).count();
    if count == 0 {
        return Ok((text.to_string(), 0));
    }
    let replaced = if query.regex {
        re.replace_all(text, replacement).into_owned()
    } else {
        // Escape any `$` in a literal replacement to avoid expansion.
        let escaped = replacement.replace('$', "$$");
        re.replace_all(text, escaped.as_str()).into_owned()
    };
    Ok((replaced, count))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_case_insensitive_and_sensitive() {
        let text = "Raven raven RAVEN crow";
        let query = SearchQuery::new("raven");
        assert_eq!(find_all(text, &query, 100).unwrap().len(), 3);
        let query = SearchQuery {
            case_sensitive: true,
            ..SearchQuery::new("raven")
        };
        let ranges = find_all(text, &query, 100).unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(&text[ranges[0].clone()], "raven");
    }

    #[test]
    fn whole_word_filtering() {
        let text = "cat catalog concat cat";
        let mut query = SearchQuery::new("cat");
        query.whole_word = true;
        let ranges = find_all(text, &query, 100).unwrap();
        assert_eq!(ranges.len(), 2);
    }

    #[test]
    fn regex_mode_with_groups() {
        let text = "fn alpha() {}\nfn beta() {}";
        let mut query = SearchQuery::new(r"fn (\w+)\(");
        query.regex = true;
        let ranges = find_all(text, &query, 100).unwrap();
        assert_eq!(ranges.len(), 2);
        let (replaced, count) = replace_all(text, &query, "fn new_$1(", 1000).unwrap();
        assert_eq!(count, 2);
        assert!(replaced.contains("fn new_alpha("));
        assert!(replaced.contains("fn new_beta("));
    }

    #[test]
    fn literal_replacement_has_no_dollar_expansion() {
        let text = "a b a";
        let query = SearchQuery::new("a");
        let (replaced, count) = replace_all(text, &query, "$1", 100).unwrap();
        assert_eq!(count, 2);
        assert_eq!(replaced, "$1 b $1");
    }

    #[test]
    fn find_next_wraps_and_find_previous() {
        let text = "one two one two";
        let query = SearchQuery::new("two");
        let next = find_next(text, &query, 14, true).unwrap().unwrap();
        assert_eq!(&text[next.clone()], "two");
        let prev = find_previous(text, &query, 8, false).unwrap().unwrap();
        assert_eq!(prev.start, 4);
        // Without wrap, nothing before position 1.
        assert!(find_previous(text, &query, 1, false).unwrap().is_none());
        assert!(find_previous(text, &query, 1, true).unwrap().is_some());
    }

    #[test]
    fn empty_pattern_and_bad_regex_are_errors() {
        assert!(SearchQuery::new("").compile().is_err());
        let mut query = SearchQuery::new("([unclosed");
        query.regex = true;
        assert!(query.compile().is_err());
    }

    #[test]
    fn unicode_offsets_stay_byte_accurate() {
        let text = "héllo wörld héllo";
        let query = SearchQuery::new("wörld");
        let ranges = find_all(text, &query, 10).unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(&text[ranges[0].clone()], "wörld");
    }
}
