//! Query classification: invoice number or text.

/// What a search box query means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Query {
    /// Blank after trimming.
    Empty,
    /// Only ASCII digits that fit in `i64`: an invoice number ("Nr. Porosie").
    InvoiceNumber(i64),
    /// Anything else, trimmed.
    Text(String),
}

/// Classifies a search box query. `"1023"` is `InvoiceNumber(1023)`; `"12a"`,
/// `"+12"` and digits that overflow `i64` are text.
pub fn classify_query(input: &str) -> Query {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Query::Empty;
    }
    if trimmed.bytes().all(|b| b.is_ascii_digit())
        && let Ok(number) = trimmed.parse::<i64>()
    {
        return Query::InvoiceNumber(number);
    }
    Query::Text(trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_queries() {
        assert_eq!(classify_query("1023"), Query::InvoiceNumber(1023));
        assert_eq!(classify_query("  0042 "), Query::InvoiceNumber(42));
        assert_eq!(classify_query("12a"), Query::Text("12a".to_owned()));
        assert_eq!(classify_query("+12"), Query::Text("+12".to_owned()));
        assert_eq!(classify_query("-1"), Query::Text("-1".to_owned()));
        assert_eq!(classify_query("1 2"), Query::Text("1 2".to_owned()));
        assert_eq!(classify_query("١٢"), Query::Text("١٢".to_owned()));
        assert_eq!(
            classify_query("9223372036854775807"),
            Query::InvoiceNumber(i64::MAX)
        );
        assert_eq!(
            classify_query("9223372036854775808"),
            Query::Text("9223372036854775808".to_owned())
        );
        assert_eq!(classify_query(""), Query::Empty);
        assert_eq!(classify_query(" \t "), Query::Empty);
        assert_eq!(classify_query(" brizo "), Query::Text("brizo".to_owned()));
    }
}
