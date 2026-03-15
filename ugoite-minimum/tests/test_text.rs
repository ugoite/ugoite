use ugoite_minimum::text::compute_word_count;

#[test]
/// REQ-IDX-005
fn test_text_req_idx_005_word_count_portable() {
    assert_eq!(compute_word_count("One two three"), 3);
    assert_eq!(compute_word_count("  tabs\tand\nnewlines  still count "), 5);
}
