//! test262 YAML frontmatter parser — a deliberately small, strict subset.
//!
//! test262 test files carry metadata between `/*---` and `---*/` markers as YAML. The
//! runner needs only a bounded fragment of YAML: top-level `key: value` scalars, flow
//! sequences (`flags: [raw, async]`), block sequences (`includes:` + indented `- item`),
//! literal/folded block scalars (`info: |`), and exactly one level of nested scalar maps
//! (`negative: {phase, type}`). Anything more complex is a hard [`MetaError`], never a
//! silent partial parse — a test whose metadata we cannot read is *not run*, because
//! misread flags change verdicts.

use std::collections::BTreeMap;

/// One parsed frontmatter value.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum YamlValue {
    Scalar(String),
    List(Vec<String>),
    Map(BTreeMap<String, String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetaError(pub String);

impl std::fmt::Display for MetaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "frontmatter error: {}", self.0)
    }
}
impl std::error::Error for MetaError {}

/// `negative:` frontmatter — the phase at which the test must fail, and the error type.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Negative {
    pub phase: String,
    #[serde(rename = "type")]
    pub error_type: Option<String>,
}

/// Typed view over a test file's frontmatter. Unrecognized keys are preserved raw so
/// tooling can inspect them without this struct growing a field per test262 key.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TestMeta {
    pub description: Option<String>,
    pub info: Option<String>,
    pub esid: Option<String>,
    pub flags: Vec<String>,
    pub includes: Vec<String>,
    pub features: Vec<String>,
    pub negative: Option<Negative>,
    /// Every top-level key as parsed, including ones without a typed field above.
    pub raw: BTreeMap<String, YamlValue>,
}

impl TestMeta {
    /// The harness include files this test needs, applying test262's default rule:
    /// a test without `flags: [raw]` implicitly includes `sta.js` and `assert.js`.
    pub fn required_includes(&self) -> Vec<String> {
        let mut out = Vec::new();
        if !self.flags.iter().any(|f| f == "raw") {
            out.push("sta.js".to_string());
            out.push("assert.js".to_string());
        }
        for inc in &self.includes {
            if !out.contains(inc) {
                out.push(inc.clone());
            }
        }
        out
    }

    pub fn has_flag(&self, flag: &str) -> bool {
        self.flags.iter().any(|f| f == flag)
    }
}

/// Extract the raw `/*--- ... ---*/` block and parse it.
pub fn parse_frontmatter(src: &str) -> Result<TestMeta, MetaError> {
    let start = src
        .find("/*---")
        .ok_or_else(|| MetaError("missing /*--- frontmatter opener".into()))?;
    let inner_start = start + "/*---".len();
    let rest = &src[inner_start..];
    let end = rest
        .find("---*/")
        .ok_or_else(|| MetaError("missing ---*/ frontmatter closer".into()))?;
    let yaml = &rest[..end];
    let raw = parse_yaml_subset(yaml)?;
    Ok(from_raw(raw))
}

/// Parse the supported YAML subset into a top-level key map.
fn parse_yaml_subset(yaml: &str) -> Result<BTreeMap<String, YamlValue>, MetaError> {
    let lines: Vec<&str> = yaml.lines().collect();
    let mut map = BTreeMap::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        i += 1;
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with(char::is_whitespace) {
            return Err(MetaError(format!(
                "unexpected indented line at top level: {line:?}"
            )));
        }
        let (key, rest) = line.split_once(':').ok_or_else(|| {
            MetaError(format!("top-level line has no `key:` form: {line:?}"))
        })?;
        let key = key.trim().to_string();
        if key.is_empty() {
            return Err(MetaError("empty top-level key".into()));
        }
        let rest = rest.trim();
        if rest.is_empty() {
            // Either a block sequence, a nested scalar map, or an empty value — decided
            // by the indented lines that follow.
            let mut block: Vec<&str> = Vec::new();
            while i < lines.len() && (lines[i].starts_with(char::is_whitespace) || lines[i].trim().is_empty())
            {
                if !lines[i].trim().is_empty() {
                    block.push(lines[i]);
                }
                i += 1;
            }
            map.insert(key, parse_indented_block(&block)?);
        } else if rest == "|" || rest == "|-" || rest == ">" || rest == ">-" {
            // Block scalar: consume the indented lines verbatim (folded `>` is joined
            // with spaces; literal `|` keeps newlines). Trailing `-` strips the final
            // newline; we always strip — exact text is informational only.
            let folded = rest.starts_with('>');
            let mut block: Vec<&str> = Vec::new();
            while i < lines.len() && (lines[i].starts_with(char::is_whitespace) || lines[i].trim().is_empty())
            {
                block.push(lines[i]);
                i += 1;
            }
            let indent = block
                .iter()
                .filter(|l| !l.trim().is_empty())
                .map(|l| l.len() - l.trim_start().len())
                .min()
                .unwrap_or(0);
            let stripped: Vec<String> = block
                .iter()
                .map(|l| l.get(indent..).unwrap_or("").to_string())
                .collect();
            let text = if folded {
                stripped.join(" ")
            } else {
                stripped.join("\n")
            };
            map.insert(key, YamlValue::Scalar(text.trim_end().to_string()));
        } else if rest.starts_with('[') {
            // Flow sequence: must close on the same line in our subset.
            if !rest.ends_with(']') {
                return Err(MetaError(format!(
                    "flow sequence for key {key:?} does not close on its line"
                )));
            }
            let inner = &rest[1..rest.len() - 1];
            let items = inner
                .split(',')
                .map(|s| unquote(s.trim()))
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>();
            map.insert(key, YamlValue::List(items));
        } else {
            map.insert(key, YamlValue::Scalar(unquote(rest)));
        }
    }
    Ok(map)
}

/// Parse an indented block under a key with no inline value: a `- item` sequence or a
/// one-level `sub: value` map (both with the common indentation stripped).
fn parse_indented_block(block: &[&str]) -> Result<YamlValue, MetaError> {
    if block.is_empty() {
        return Ok(YamlValue::Scalar(String::new()));
    }
    let indent = block
        .iter()
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    let stripped: Vec<&str> = block.iter().map(|l| l.get(indent..).unwrap_or("")).collect();
    if stripped.iter().all(|l| l.starts_with("- ") || *l == "-") {
        let items = stripped
            .iter()
            .map(|l| unquote(l.trim_start_matches('-').trim()))
            .collect::<Vec<_>>();
        return Ok(YamlValue::List(items));
    }
    if stripped.iter().all(|l| l.contains(": ")) {
        let mut map = BTreeMap::new();
        for l in stripped {
            let (k, v) = l.split_once(':').expect("checked contains");
            map.insert(k.trim().to_string(), unquote(v.trim()));
        }
        return Ok(YamlValue::Map(map));
    }
    Err(MetaError(format!(
        "indented block is neither a sequence nor a scalar map: {stripped:?}"
    )))
}

fn unquote(s: &str) -> String {
    let bytes = s.as_bytes();
    if bytes.len() >= 2
        && ((bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\''))
    {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

fn from_raw(raw: BTreeMap<String, YamlValue>) -> TestMeta {
    let scalar = |k: &str| match raw.get(k) {
        Some(YamlValue::Scalar(s)) if !s.is_empty() => Some(s.clone()),
        _ => None,
    };
    let list = |k: &str| match raw.get(k) {
        Some(YamlValue::List(v)) => v.clone(),
        _ => Vec::new(),
    };
    let negative = match raw.get("negative") {
        Some(YamlValue::Map(m)) => m.get("phase").map(|phase| Negative {
            phase: phase.clone(),
            error_type: m.get("type").cloned(),
        }),
        _ => None,
    };
    TestMeta {
        description: scalar("description"),
        info: scalar("info"),
        esid: scalar("esid").or_else(|| scalar("es5id")).or_else(|| scalar("es6id")),
        flags: list("flags"),
        includes: list("includes"),
        features: list("features"),
        negative,
        raw,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_typical_frontmatter() {
        let src = r#"// comment
/*---
es5id: 7.8.2_A1_T1
description: "BooleanLiteral :: true"
info: |
    Multi-line
    literal block.
flags: [raw, noStrict]
includes:
  - propertyHelper.js
features:
  - BigInt
negative:
  phase: parse
  type: SyntaxError
---*/
var x = true;
"#;
        let meta = parse_frontmatter(src).unwrap();
        assert_eq!(meta.description.as_deref(), Some("BooleanLiteral :: true"));
        assert_eq!(meta.info.as_deref(), Some("Multi-line\nliteral block."));
        assert_eq!(meta.flags, vec!["raw", "noStrict"]);
        assert_eq!(meta.includes, vec!["propertyHelper.js"]);
        assert_eq!(meta.features, vec!["BigInt"]);
        assert_eq!(
            meta.negative,
            Some(Negative {
                phase: "parse".into(),
                error_type: Some("SyntaxError".into())
            })
        );
        // Default includes apply only without `raw`.
        assert_eq!(meta.required_includes(), vec!["propertyHelper.js"]);
    }

    #[test]
    fn default_includes_without_raw_flag() {
        let meta = parse_frontmatter("/*---\ndescription: x\n---*/").unwrap();
        assert_eq!(meta.required_includes(), vec!["sta.js", "assert.js"]);
    }


    #[test]
    fn time_octal_frontmatter() {
        let src = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../vendor/test262/test/language/literals/numeric/octal.js"),
        )
        .unwrap();
        let t = std::time::Instant::now();
        let m = parse_frontmatter(&src).unwrap();
        eprintln!("meta: {:?} flags={:?}", t.elapsed(), m.flags);
    }
    #[test]
    fn rejects_garbage() {
        assert!(parse_frontmatter("no frontmatter here").is_err());
        assert!(parse_frontmatter("/*---\n  indented: badly\n---*/").is_err());
    }
}
