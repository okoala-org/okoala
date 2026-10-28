// Copyright (C) 2026 IASoft PVT LTD
// SPDX-License-Identifier: AGPL-3.0-only

//! `check-spdx`: every tracked `.rs` file starts with the two-line copyright and licence header.

const LINE2: &str = "// SPDX-License-Identifier: AGPL-3.0-only";

fn copyright_ok(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("// Copyright (C) ") else {
        return false;
    };
    let Some((year, owner)) = rest.split_once(' ') else {
        return false;
    };
    year.len() == 4 && year.bytes().all(|b| b.is_ascii_digit()) && owner == "IASoft PVT LTD"
}

/// True when `text` starts with the header (a trailing `\r` is tolerated).
pub fn has_header(text: &str) -> bool {
    let mut lines = text.lines();
    matches!((lines.next(), lines.next()), (Some(a), Some(b)) if copyright_ok(a) && b == LINE2)
}

pub fn run(root: &std::path::Path) -> Result<(), String> {
    let mut bad = Vec::new();
    let mut count = 0usize;
    for path in crate::git::ls_files(root)? {
        if !path.ends_with(".rs") {
            continue;
        }
        count += 1;
        let text = std::fs::read_to_string(root.join(&path)).map_err(|e| format!("{path}: {e}"))?;
        if !has_header(&text) {
            bad.push(path);
        }
    }
    if bad.is_empty() {
        println!("check-spdx: ok ({count} .rs files)");
        return Ok(());
    }
    for p in &bad {
        eprintln!("missing or wrong SPDX header: {p}");
    }
    Err(format!(
        "check-spdx: {} file(s) without the header",
        bad.len()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_header_with_any_year() {
        assert!(has_header(
            "// Copyright (C) 2026 IASoft PVT LTD\n// SPDX-License-Identifier: AGPL-3.0-only\n\nfn main() {}\n"
        ));
        assert!(has_header(
            "// Copyright (C) 2031 IASoft PVT LTD\n// SPDX-License-Identifier: AGPL-3.0-only"
        ));
        assert!(has_header(
            "// Copyright (C) 2026 IASoft PVT LTD\r\n// SPDX-License-Identifier: AGPL-3.0-only\r\n"
        ));
    }

    #[test]
    fn rejects_bad_headers() {
        assert!(!has_header(""));
        assert!(!has_header("fn main() {}\n"));
        assert!(!has_header("// Copyright (C) 2026 IASoft PVT LTD\n"));
        assert!(!has_header(
            "// SPDX-License-Identifier: AGPL-3.0-only\n// Copyright (C) 2026 IASoft PVT LTD\n"
        ));
        assert!(!has_header(
            "// Copyright (C) 26 IASoft PVT LTD\n// SPDX-License-Identifier: AGPL-3.0-only\n"
        ));
        assert!(!has_header(
            "// Copyright (C) 2026 Someone Else\n// SPDX-License-Identifier: AGPL-3.0-only\n"
        ));
        assert!(!has_header(
            "// Copyright (C) 2026 IASoft PVT LTD\n// SPDX-License-Identifier: MIT\n"
        ));
        assert!(!has_header(
            "\n// Copyright (C) 2026 IASoft PVT LTD\n// SPDX-License-Identifier: AGPL-3.0-only\n"
        ));
    }
}
