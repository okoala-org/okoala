// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! Equality on `Secret` must give the same answers as comparing the bytes directly, for
//! every comparison form the crate offers. These tests check correctness only; they
//! cannot show that the comparison takes constant time.

use okoala_secret::Secret;

fn s(value: &str) -> Secret<String> {
    Secret::new(String::from(value))
}

#[test]
fn equal_values_are_equal() {
    assert!(s("token-123") == s("token-123"));
    assert!(s("") == s(""));
}

#[test]
fn different_values_are_not_equal() {
    assert!(s("token-123") != s("token-124"));
    assert!(s("Token-123") != s("token-123"));
}

#[test]
fn values_differing_only_at_one_end_are_not_equal() {
    assert!(s("xbcdef") != s("abcdef"), "first byte differs");
    assert!(s("abcdex") != s("abcdef"), "last byte differs");
}

#[test]
fn different_lengths_are_not_equal() {
    assert!(s("abc") != s("abcd"), "prefix");
    assert!(s("abcd") != s("abc"), "extension");
    assert!(s("") != s("a"), "empty against non-empty");
    assert!(s("a") != s(""), "non-empty against empty");
}

#[test]
fn trailing_nul_bytes_are_not_ignored() {
    assert!(s("abc") != s("abc\0"));
    assert!(s("abc\0") != s("abc"));
}

#[test]
fn comparison_is_by_bytes_not_by_unicode_meaning() {
    // "é" precomposed (U+00E9) and decomposed (e + U+0301) look the same but are
    // different bytes; secrets must compare byte for byte.
    assert!(s("caf\u{e9}") != s("cafe\u{301}"));
    assert!(s("caf\u{e9}") == s("caf\u{e9}"));
}

#[test]
fn equality_is_reflexive_and_symmetric() {
    let a = s("same");
    let b = s("same");
    let c = s("other");
    assert!(a == a);
    assert_eq!(a == b, b == a);
    assert_eq!(a == c, c == a);
}

#[test]
fn byte_secrets_compare_correctly() {
    let a = Secret::new(vec![1u8, 2, 3]);
    assert!(a == Secret::new(vec![1u8, 2, 3]));
    assert!(a != Secret::new(vec![1u8, 2, 4]));
    assert!(a != Secret::new(vec![1u8, 2]));
    assert!(Secret::new(Vec::<u8>::new()) == Secret::new(Vec::<u8>::new()));

    let key = Secret::new([9u8; 32]);
    assert!(key == Secret::new([9u8; 32]));
    let mut other = [9u8; 32];
    other[31] = 8;
    assert!(key != Secret::new(other));
}

#[test]
fn comparison_with_str_forms() {
    let secret = s("token-123");

    assert!(secret == "token-123");
    assert!(secret != "token-124");
    assert!(secret != "token-12");
    assert!(secret != "");

    assert!(secret == *"token-123");
    assert!(secret != *"token-1234");

    assert!("token-123" == secret);
    assert!("token-124" != secret);
    assert!("" != secret);
}

#[test]
fn comparison_with_byte_slice_forms() {
    let secret = Secret::new(vec![1u8, 2, 3]);
    let same: &[u8] = &[1, 2, 3];
    let different: &[u8] = &[1, 2, 4];
    let shorter: &[u8] = &[1, 2];

    assert!(secret == same);
    assert!(secret != different);
    assert!(secret != shorter);

    assert!(secret == *same);
    assert!(secret != *different);
    assert!(secret != *shorter);
}

#[test]
fn string_secret_compares_with_its_bytes_and_byte_secret_with_text() {
    assert!(s("abc") == b"abc".as_slice());
    assert!(Secret::new(b"abc".to_vec()) == "abc");
}

/// Compares every pair from a set of tricky inputs against plain byte equality, through
/// every comparison form, so that no form disagrees with the others.
#[test]
fn every_form_agrees_with_plain_byte_equality() {
    let inputs = [
        "",
        "a",
        "b",
        "ab",
        "ba",
        "abc",
        "abd",
        "abc\0",
        "\0",
        "\0\0",
        "token-123",
        "token-124",
        "Token-123",
        "caf\u{e9}",
        "cafe\u{301}",
        "\u{1F428}",
    ];
    for left in inputs {
        for right in inputs {
            let expected = left.as_bytes() == right.as_bytes();
            let secret = s(left);
            assert_eq!(
                secret == s(right),
                expected,
                "Secret == Secret: {left:?} {right:?}"
            );
            assert_eq!(
                secret == right,
                expected,
                "Secret == &str: {left:?} {right:?}"
            );
            assert_eq!(
                secret == *right,
                expected,
                "Secret == str: {left:?} {right:?}"
            );
            assert_eq!(
                right == secret,
                expected,
                "&str == Secret: {left:?} {right:?}"
            );
            assert_eq!(
                secret == right.as_bytes(),
                expected,
                "Secret == &[u8]: {left:?} {right:?}"
            );
            assert_eq!(
                secret == *right.as_bytes(),
                expected,
                "Secret == [u8]: {left:?} {right:?}"
            );
            assert_eq!(secret != s(right), !expected, "!= is the negation of ==");
        }
    }
}
