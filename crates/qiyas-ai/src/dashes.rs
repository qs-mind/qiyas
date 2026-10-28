pub(crate) fn plain_dashes(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '—' | '–' | '―' => '-',
            _ => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::plain_dashes;

    #[test]
    fn replaces_long_dashes_without_changing_spaces_or_other_text() {
        assert_eq!(plain_dashes("A — B – C―D"), "A - B - C-D");
        assert_eq!(plain_dashes("قياس — 12–15"), "قياس - 12-15");
        assert_eq!(plain_dashes("  x-y\n"), "  x-y\n");
        assert_eq!(plain_dashes(""), "");
        let deltas = [" —", "— x", "–", "― "];
        assert_eq!(deltas.map(plain_dashes).concat(), " -- x-- ");
    }
}
