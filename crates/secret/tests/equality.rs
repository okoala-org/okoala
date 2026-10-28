// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! `Secret::compare`, `Secret::compare_value` and `Secret::compare_bytes` must give the
//! same answers as comparing the bytes directly. These tests check correctness only; they
//! cannot show that the comparison takes constant time.

use okoala_secret::Secret;

fn s(value: &str) -> Secret<String> {
    Secret::new(String::from(value))
}

/// Compares through all three methods and checks that they agree.
fn same(left: &str, right: &str) -> bool {
    let secret = s(left);
    let by_secret = secret.compare(&s(right));
    let by_value = secret.compare_value(&String::from(right));
    let by_bytes = secret.compare_bytes(right.as_bytes());
    assert_eq!(
        by_secret, by_value,
        "compare and compare_value disagree: {left:?} {right:?}"
    );
    assert_eq!(
        by_secret, by_bytes,
        "compare and compare_bytes disagree: {left:?} {right:?}"
    );
    by_secret
}

#[test]
fn equal_values_are_equal() {
    assert!(same("token-123", "token-123"));
}

/// Review finding 1: a credential that is unset (an empty string from a missing setting
/// or `unwrap_or_default()`) must not let an empty token in.
#[test]
fn an_empty_secret_matches_nothing() {
    for stored in [s(""), Secret::new(String::new())] {
        assert!(!stored.compare(&s("")), "empty against empty secret");
        assert!(
            !stored.compare_value(&String::new()),
            "empty against empty value"
        );
        assert!(!stored.compare_bytes(b""), "empty against empty bytes");
        assert!(!stored.compare_bytes(b"x"));
        assert!(!stored.compare(&s("token-123")));
    }
    let empty_bytes = Secret::new(Vec::<u8>::new());
    assert!(!empty_bytes.compare(&Secret::new(Vec::new())));
    assert!(!empty_bytes.compare_value(&Vec::new()));
    assert!(!empty_bytes.compare_bytes(&[]));
    assert!(!Secret::new(Box::<[u8]>::from(&[][..])).compare_bytes(&[]));
    assert!(!Secret::new([0u8; 0]).compare_bytes(&[]));
}

#[test]
fn an_empty_bearer_token_is_rejected() {
    let unset = Secret::new(std::env::var("OKOALA_TEST_UNSET_SECRET").unwrap_or_default());
    let header = "Bearer ";
    let presented = header.strip_prefix("Bearer ").unwrap_or_default();
    assert!(presented.is_empty());
    assert!(!unset.compare_bytes(presented.as_bytes()));
    assert!(!s("token-123").compare_bytes(presented.as_bytes()));
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
    assert!(!secret.compare_value(&String::from("wrong")));
    assert!(secret.compare_value(&String::from("token-123")));
    assert!(secret.compare(&s("token-123")));
    assert_eq!(secret.reveal_value(), "token-123");
}

#[test]
fn both_secrets_stay_usable_after_comparing() {
    let stored = s("token-123");
    let presented = s("token-123");
    assert!(stored.compare(&presented));
    assert!(presented.compare(&stored));
    assert_eq!(stored.reveal_value(), "token-123");
    assert_eq!(presented.reveal_value(), "token-123");

    let expected = String::from("token-123");
    assert!(stored.compare_value(&expected));
    assert_eq!(expected, "token-123", "the compared value is only borrowed");
}

#[test]
fn byte_secrets_compare_correctly() {
    let a = Secret::new(vec![1u8, 2, 3]);
    assert!(a.compare(&Secret::new(vec![1u8, 2, 3])));
    assert!(!a.compare(&Secret::new(vec![1u8, 2, 4])));
    assert!(!a.compare(&Secret::new(vec![1u8, 2])));
    assert!(a.compare_value(&vec![1u8, 2, 3]));
    assert!(!a.compare_value(&vec![1u8, 2, 3, 4]));
    assert!(a.compare_bytes(&[1, 2, 3]));
    assert!(!a.compare_bytes(&[1, 2, 4]));
    assert!(!a.compare_bytes(&[1, 2]));
    assert!(!a.compare_bytes(&[]));
}

/// The case `compare_bytes` exists for: a token from a request arrives borrowed (`&str`,
/// a slice of a header or a buffer) and is checked without copying it into an owned
/// value that would never be zeroized.
#[test]
fn compare_bytes_checks_borrowed_input_without_copying() {
    let stored = s("token-123");
    let header = "Bearer token-123";
    let presented: &str = header.strip_prefix("Bearer ").unwrap_or_default();
    assert!(stored.compare_bytes(presented.as_bytes()));
    assert!(!stored.compare_bytes(&header.as_bytes()[..9]));
    assert!(!stored.compare_bytes(header.as_bytes()));

    let buffer: [u8; 12] = *b"token-123xyz";
    assert!(stored.compare_bytes(&buffer[..9]));
    assert!(!stored.compare_bytes(&buffer[..10]));
    assert!(!stored.compare_bytes(&buffer));
}

#[test]
fn compare_bytes_compares_text_secrets_with_bytes_and_byte_secrets_with_text() {
    assert!(s("abc").compare_bytes(b"abc"));
    assert!(!s("abc").compare_bytes(b"ABC"));
    assert!(Secret::new(b"abc".to_vec()).compare_bytes("abc".as_bytes()));
    assert!(Secret::new([0u8; 4]).compare_bytes(&[0, 0, 0, 0]));
    assert!(!Secret::new([0u8; 4]).compare_bytes(&[0, 0, 0]));
}

#[test]
fn fixed_size_keys_compare_correctly() {
    let key = Secret::new([9u8; 32]);
    assert!(key.compare(&Secret::new([9u8; 32])));
    assert!(key.compare_value(&[9u8; 32]));
    let mut other = [9u8; 32];
    other[31] = 8;
    assert!(!key.compare_value(&other));
    other[31] = 9;
    other[0] = 8;
    assert!(!key.compare(&Secret::new(other)));
}

/// Compares every pair from a set of tricky inputs against plain byte equality (an empty
/// stored secret never matches), through all three methods and for `String` and `Vec<u8>`
/// values.
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
            // An empty stored secret never matches (review finding 1).
            let expected = !left.is_empty() && left.as_bytes() == right.as_bytes();
            assert_eq!(same(left, right), expected, "String: {left:?} {right:?}");

            let bytes = Secret::new(left.as_bytes().to_vec());
            assert_eq!(
                bytes.compare(&Secret::new(right.as_bytes().to_vec())),
                expected,
                "Vec<u8> compare: {left:?} {right:?}"
            );
            assert_eq!(
                bytes.compare_value(&right.as_bytes().to_vec()),
                expected,
                "Vec<u8> compare_value: {left:?} {right:?}"
            );
            assert_eq!(
                bytes.compare_bytes(right.as_bytes()),
                expected,
                "Vec<u8> compare_bytes: {left:?} {right:?}"
            );
        }
    }
}
