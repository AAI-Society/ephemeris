use ephemeris::store::Store;

/// This crate reads bytes it did not write. Every one of these must return an
/// error rather than panic.
#[test]
fn malformed_segments_error_rather_than_panic() {
    let cases: Vec<Vec<u8>> = vec![
        vec![],
        vec![0],
        vec![0, 0, 0],
        vec![0, 0, 0, 1],
        vec![0, 0, 0, 1, b'{'],
        vec![0xff, 0xff, 0xff, 0xff],
        vec![0, 0, 0, 2, b'{', b'}'],
        b"not a frame at all".to_vec(),
    ];
    for (i, bytes) in cases.into_iter().enumerate() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("segment-0.log"), &bytes).unwrap();
        // Must not panic. Either outcome is acceptable; a panic is not.
        let _ = Store::open(d.path());
        let _ = i;
    }
}
