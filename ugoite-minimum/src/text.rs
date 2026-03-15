pub fn compute_word_count(content: &str) -> usize {
    content.split_whitespace().count()
}
