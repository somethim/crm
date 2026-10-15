//! SKU code helpers.

use crate::AUTO_CODE_PREFIX;

/// The base SKU code a product code belongs to: trims, then strips one trailing
/// `/<digits>` flavour suffix. `"L0009/3"` → `"L0009"`, `"A/B"` stays `"A/B"`.
pub fn base_code(code: &str) -> &str {
    let code = code.trim();
    match code.rsplit_once('/') {
        Some((base, suffix))
            if !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit()) =>
        {
            base
        }
        _ => code,
    }
}

/// The next automatic code (001 A10): one more than the largest existing
/// `A` + 4-or-more digits code (`A0001` when there is none). Other codes are
/// ignored. Past `A9999` the number simply grows (`A10000`, then `A10001`).
pub fn next_auto_code<'a>(existing: impl IntoIterator<Item = &'a str>) -> String {
    let max = existing
        .into_iter()
        .filter_map(|code| auto_number(base_code(code)))
        .max()
        .unwrap_or(0);
    format!("{AUTO_CODE_PREFIX}{:04}", max.saturating_add(1))
}

fn auto_number(code: &str) -> Option<u64> {
    let digits = code.strip_prefix(AUTO_CODE_PREFIX)?;
    if digits.len() >= 4 && digits.bytes().all(|b| b.is_ascii_digit()) {
        digits.parse().ok()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_codes() {
        assert_eq!(base_code("L0009/3"), "L0009");
        assert_eq!(base_code(" A0002 "), "A0002");
        assert_eq!(base_code("A/B"), "A/B");
        assert_eq!(base_code("L0009/"), "L0009/");
        assert_eq!(base_code("X/1/2"), "X/1");
        assert_eq!(base_code("L0009/12"), "L0009");
        assert_eq!(base_code("L0009"), "L0009");
    }

    #[test]
    fn auto_codes() {
        assert_eq!(next_auto_code([]), "A0001");
        assert_eq!(next_auto_code(["L0009", "A/B", "B0005"]), "A0001");
        assert_eq!(next_auto_code(["A0002", "A0009", "A0003"]), "A0010");
        assert_eq!(
            next_auto_code(["A0005/1", "A001", "AB123", "a0099"]),
            "A0006"
        );
        assert_eq!(next_auto_code(["A9999"]), "A10000");
        assert_eq!(next_auto_code(["A9999", "A10000"]), "A10001");
        let owned = [String::from("A0041")];
        assert_eq!(next_auto_code(owned.iter().map(String::as_str)), "A0042");
    }
}
