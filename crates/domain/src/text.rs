//! Albanian-aware text folding for search keys (`name_key`, `search_key`).

/// Lower-cases, strips diacritics (`ë→e`, `ç→c`, common Latin accents),
/// collapses whitespace and trims. `"Brizo Pjeshkë ÇAJ"` → `"brizo pjeshke caj"`.
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending_space = false;
    for c in s.chars().flat_map(char::to_lowercase) {
        if c.is_whitespace() {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.push(strip_diacritic(c));
    }
    out
}

/// Folded words: [`fold`], then split on every non-alphanumeric character.
pub fn tokens(s: &str) -> Vec<String> {
    fold(s)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .collect()
}

fn strip_diacritic(c: char) -> char {
    match c {
        'á' | 'à' | 'â' | 'ä' => 'a',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'í' | 'ì' | 'î' | 'ï' => 'i',
        'ó' | 'ò' | 'ô' | 'ö' => 'o',
        'ú' | 'ù' | 'û' | 'ü' => 'u',
        'ç' => 'c',
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_examples() {
        assert_eq!(fold("Brizo Pjeshkë ÇAJ"), "brizo pjeshke caj");
        assert_eq!(fold("  Çokollatë   Ë  "), "cokollate e");
        assert_eq!(fold("Café Crème Brûlée"), "cafe creme brulee");
        assert_eq!(
            fold("Ìì Íí Îî Ïï Óò Ôö Úù Ûü Áà Âä"),
            "ii ii ii ii oo oo uu uu aa aa"
        );
        assert_eq!(fold("A\t\nB"), "a b");
        assert_eq!(fold(""), "");
        assert_eq!(fold("   "), "");
    }

    #[test]
    fn tokens_split_on_punctuation() {
        assert_eq!(
            tokens("BRIZO PJESHKË 24*0.33LIT"),
            ["brizo", "pjeshke", "24", "0", "33lit"]
        );
        assert_eq!(tokens(" ... "), Vec::<String>::new());
    }
}
