//! The property the design asks for: a build without the `tee` feature cannot
//! emit a root claiming hardware custody.

use ephemeris::sign::{Custody, Signer, SoftwareSigner};

#[test]
fn a_default_build_signs_with_software_custody() {
    let s = SoftwareSigner::generate([7u8; 32]);
    assert_eq!(s.custody(), Custody::Software);
    assert_eq!(s.sign_root(&[0u8; 32], 1).custody, Custody::Software);
}

#[test]
fn a_rendered_root_says_software_and_says_what_is_undefended() {
    let text = SoftwareSigner::generate([7u8; 32])
        .sign_root(&[0u8; 32], 1)
        .render();
    assert!(text.contains("SOFTWARE"));
    assert!(
        !text.contains("TEE"),
        "a software build rendered a TEE claim"
    );
    assert!(text.contains("C7.3.3"));
}

/// Compile-fail is the real assertion and it cannot be written as a runtime
/// test: without the `tee` feature there is no expression that produces
/// `Custody::Tee`. This test documents the property and fails loudly if a
/// future edit adds a public constructor reachable from a default build.
///
/// A naive per-line scan (does *this line* contain `=>` or the cfg text
/// itself?) does not survive the constructor the property requires:
///
/// ```ignore
/// #[cfg(feature = "tee")]
/// pub(crate) fn tee() -> Custody {
///     Custody::Tee
/// }
/// ```
///
/// The line that actually yields `Custody::Tee` is the function body, which
/// has neither a `=>` nor the `cfg` attribute on it — the attribute is on the
/// line above, gating the enclosing item rather than the line itself. So this
/// walks brace depth to know, for every line, whether it sits inside an item
/// whose *nearest preceding attribute* was `#[cfg(feature = "tee")]`, and
/// only then allows a bare `Custody::Tee` outside a match arm. The property
/// under test is unchanged: no default-build-reachable expression yields
/// `Custody::Tee`. What changed is recognizing "reachable" correctly for an
/// attribute that gates a whole item rather than one line.
#[test]
fn nothing_in_a_default_build_constructs_tee_custody() {
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/sign.rs"))
        .expect("sign.rs is part of this crate");

    // `depth` is brace nesting. `gate_active`/`gate_depth` track whether the
    // item we are currently inside was introduced by a
    // `#[cfg(feature = "tee")]` attribute, and at what depth that item's body
    // began (so we know when it closes).
    let mut depth: i32 = 0;
    let mut pending_gate = false;
    let mut gate_active = false;
    let mut gate_depth: i32 = 0;

    for (i, line) in src.lines().enumerate() {
        let code = line.split("//").next().unwrap_or("").trim();

        if code.contains("Custody::Tee") {
            let in_match_arm = code.contains("=>");
            let directly_gated = code.contains("cfg(feature = \"tee\")");
            assert!(
                in_match_arm || directly_gated || gate_active,
                "src/sign.rs:{} constructs Custody::Tee outside a match arm and outside an item gated on the tee feature: {}",
                i + 1,
                line.trim()
            );
        }

        if code.contains("cfg(feature = \"tee\")") {
            pending_gate = true;
        } else if pending_gate && !code.is_empty() && !code.starts_with('#') {
            // First substantive line after the attribute is the item it gates.
            gate_active = true;
            gate_depth = depth;
            pending_gate = false;
        }

        for ch in code.chars() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if gate_active && depth <= gate_depth {
                        gate_active = false;
                    }
                }
                _ => {}
            }
        }
    }

    assert!(
        !pending_gate,
        "a #[cfg(feature = \"tee\")] attribute at end of file was never applied to an item"
    );
}
