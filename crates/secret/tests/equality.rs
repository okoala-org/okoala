// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! `Secret::compare` and `Secret::compare_value` must give the same answers as comparing
//! the bytes directly. These tests check correctness only; they cannot show that the
//! comparison takes constant time.

use okoala_secret::Secret;

fn s(value: &str) -> Secret<String> {
    Secret::new(String::from(value))
}

/// Compares through both methods and checks that they agree.
fn same(left: &str, right: &str) -> bool {
    let by_secret = s(left).compare(s(right));
    let by_value = s(left).compare_value(String::from(right));
    assert_eq!(
        by_secret, by_value,
        "compare and compare_value disagree: {left:?} {right:?}"
    );
    by_secret
}

#[test]
fn equal_values_are_equal() {
    assert!(same("token-123", "token-123"));
    assert!(same("", ""));
}

#[test]
fn different_values_are_not_equal() {
    assert!(!same("token-123", "token-124"));
    assert!(!same("Token-123", "token-123"));
}

#[test]
fn values_differing_only_at_one_end_are_not_equal() {
    assert!(!same("xbcdef", "abcdef"), "first byte differs");
    assert!(!same("abcdex", "abcdef"), "last byte differs");
}

#[test]
fn different_lengths_are_not_equal() {
    assert!(!same("abc", "abcd"), "prefix");
    assert!(!same("abcd", "abc"), "extension");
    assert!(!same("", "a"), "empty against non-empty");
    assert!(!same("a", ""), "non-empty against empty");
}

#[test]
fn trailing_nul_bytes_are_not_ignored() {
    assert!(!same("abc", "abc\0"));
    assert!(!same("abc\0", "abc"));
}

#[test]
fn comparison_is_by_bytes_not_by_unicode_meaning() {
    // "é" precomposed (U+00E9) and decomposed (e + U+0301) look the same but are
    // different bytes; secrets must compare byte for byte.
    assert!(!same("caf\u{e9}", "cafe\u{301}"));
    assert!(same("caf\u{e9}", "caf\u{e9}"));
}

#[test]
fn comparison_is_symmetric() {
    for (a, b) in [("same", "same"), ("same", "other"), ("", "x")] {
        assert_eq!(same(a, b), same(b, a), "{a:?} {b:?}");
    }
}

#[test]
fn comparing_does_not_change_the_secret() {
    let secret = s("token-123");
    assert!(!secret.compare_value(String::from("wrong")));
    assert!(secret.compare_value(String::from("token-123")));
    assert!(secret.compare(s("token-123")));
    assert_eq!(secret.reveal_value(), "token-123");
}

#[test]
fn byte_secrets_compare_correctly() {
    let a = Secret::new(vec![1u8, 2, 3]);
    assert!(a.compare(Secret::new(vec![1u8, 2, 3])));
    assert!(!a.compare(Secret::new(vec![1u8, 2, 4])));
    assert!(!a.compare(Secret::new(vec![1u8, 2])));
    assert!(a.compare_value(vec![1u8, 2, 3]));
    assert!(!a.compare_value(vec![1u8, 2, 3, 4]));
    assert!(Secret::new(Vec::<u8>::new()).compare_value(Vec::new()));
}

#[test]
fn fixed_size_keys_compare_correctly() {
    let key = Secret::new([9u8; 32]);
    assert!(key.compare(Secret::new([9u8; 32])));
    assert!(key.compare_value([9u8; 32]));
    let mut other = [9u8; 32];
    other[31] = 8;
    assert!(!key.compare_value(other));
    other[31] = 9;
    other[0] = 8;
    assert!(!key.compare(Secret::new(other)));
}

/// Compares every pair from a set of tricky inputs against plain byte equality, through
/// both methods and for `String` and `Vec<u8>` values.
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
            assert_eq!(same(left, right), expected, "String: {left:?} {right:?}");

            let bytes = Secret::new(left.as_bytes().to_vec());
            assert_eq!(
                bytes.compare(Secret::new(right.as_bytes().to_vec())),
                expected,
                "Vec<u8> compare: {left:?} {right:?}"
            );
            assert_eq!(
                bytes.compare_value(right.as_bytes().to_vec()),
                expected,
                "Vec<u8> compare_value: {left:?} {right:?}"
            );
        }
    }
}
