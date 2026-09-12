//! Normalization of `FrontendError::Unsupported` messages into stable manifest keys.
//!
//! Some frontend messages are *families*: they embed the offending literal's value
//! (`numeric literal 0.01 (Jade's LIT32 …)` — a distinct message per source literal) or
//! a TAC `Debug` dump with register names (`Un { arg: ("$v53", #2), op: "-" }`,
//! `New { class: ("$v5", #2), args: [] }`). The capability manifest can't key those
//! verbatim (the key set would be unbounded), so the runner maps each family to a stable
//! form here; the raw message is preserved in the verdict's `error` field for debugging.

/// Map a raw `Unsupported` message to its stable manifest key.
pub fn unsupported(msg: &str) -> String {
    // `numeric literal <value> (Jade's LIT32 only represents …)` — one family.
    if let Some(rest) = msg.strip_prefix("numeric literal ") {
        if let Some(idx) = rest.find(" (Jade's LIT32") {
            return format!("numeric literal <value>{}", &rest[idx..]);
        }
    }
    // `Un { arg: (<reg>, <ctx>), op: "<op>" }` — TAC Debug dump for unary operators.
    if msg.starts_with("Un {") {
        if let Some(op) = extract_quoted_after(msg, "op: ") {
            return format!("unary operator {op:?} (no Jade opcode)");
        }
        return "unary operator (no Jade opcode)".to_string();
    }
    // `New { class: (<reg>, <ctx>), args: […] }` — `new` expressions.
    if msg.starts_with("New {") {
        return "new expressions (no Jade opcode)".to_string();
    }
    // `closure capture of `x`, `y` (see docs/…)` — names vary per test.
    if msg.starts_with("closure capture of ") {
        return "closure capture (see docs/closure-capture-plan.md)".to_string();
    }
    msg.to_string()
}

fn extract_quoted_after<'a>(msg: &'a str, needle: &str) -> Option<&'a str> {
    let start = msg.find(needle)? + needle.len();
    let rest = &msg[start..];
    if !rest.starts_with('"') {
        return None;
    }
    let end = rest[1..].find('"')? + 1;
    Some(&rest[1..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_numeric_family() {
        assert_eq!(
            unsupported("numeric literal 0.01 (Jade's LIT32 only represents non-negative integers up to u32::MAX)"),
            "numeric literal <value> (Jade's LIT32 only represents non-negative integers up to u32::MAX)"
        );
    }

    #[test]
    fn normalizes_unary_dump() {
        assert_eq!(
            unsupported("Un { arg: (\"$v53\", #2), op: \"-\" }"),
            "unary operator \"-\" (no Jade opcode)"
        );
    }

    #[test]
    fn normalizes_new_dump() {
        assert_eq!(
            unsupported("New { class: (\"$v5\", #2), args: [(\"$v6\", #2)] }"),
            "new expressions (no Jade opcode)"
        );
    }

    #[test]
    fn normalizes_capture_family() {
        assert_eq!(
            unsupported("closure capture of `x`, `y` (see docs/closure-capture-plan.md)"),
            "closure capture (see docs/closure-capture-plan.md)"
        );
    }

    #[test]
    fn stable_messages_pass_through() {
        assert_eq!(unsupported("regex literal"), "regex literal");
        assert_eq!(unsupported("tail call"), "tail call");
    }
}
