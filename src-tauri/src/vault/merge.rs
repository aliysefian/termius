//! Three-way merge of record data, field by field.
//!
//! Given the common ancestor, our version and theirs: a top-level field that
//! only one side changed takes that side's value; a field both changed to the
//! same value is fine; a field both changed differently is a conflict.
//! Nested values (such as a credential's `auth`) are compared whole, so a
//! secret is never assembled from two different edits. `tags` is merged as a
//! set, since adding different tags on two machines should keep both.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

/// Merge, or return the names of conflicting fields.
pub fn merge3(base: &Value, ours: &Value, theirs: &Value) -> Result<Value, Vec<String>> {
    if ours == theirs {
        return Ok(ours.clone());
    }
    if ours == base {
        return Ok(theirs.clone());
    }
    if theirs == base {
        return Ok(ours.clone());
    }
    let (Value::Object(b), Value::Object(o), Value::Object(t)) = (base, ours, theirs) else {
        return Err(vec!["(whole record)".into()]);
    };
    let keys: BTreeSet<&String> = b.keys().chain(o.keys()).chain(t.keys()).collect();
    let mut out = Map::new();
    let mut conflicts = Vec::new();
    for k in keys {
        let (bv, ov, tv) = (b.get(k), o.get(k), t.get(k));
        let pick = if ov == tv {
            ov
        } else if ov == bv {
            tv
        } else if tv == bv {
            ov
        } else if k == "tags" {
            match merge_string_sets(bv, ov, tv) {
                Some(v) => {
                    out.insert(k.clone(), v);
                    continue;
                }
                None => {
                    conflicts.push(k.clone());
                    continue;
                }
            }
        } else {
            conflicts.push(k.clone());
            continue;
        };
        if let Some(v) = pick {
            out.insert(k.clone(), v.clone());
        }
    }
    if conflicts.is_empty() {
        Ok(Value::Object(out))
    } else {
        Err(conflicts)
    }
}

fn strings(v: Option<&Value>) -> Option<Vec<String>> {
    match v {
        None | Some(Value::Null) => Some(Vec::new()),
        Some(Value::Array(a)) => a.iter().map(|x| x.as_str().map(str::to_string)).collect(),
        _ => None,
    }
}

/// (kept by both) ∪ (added by us) ∪ (added by them), minus anything either
/// side removed. Order: ours first, then their additions.
fn merge_string_sets(b: Option<&Value>, o: Option<&Value>, t: Option<&Value>) -> Option<Value> {
    let (b, o, t) = (strings(b)?, strings(o)?, strings(t)?);
    let removed = |x: &String, side: &[String]| b.contains(x) && !side.contains(x);
    let mut out: Vec<String> = Vec::new();
    for x in o.iter().chain(t.iter()) {
        if !out.contains(x) && !removed(x, &o) && !removed(x, &t) {
            out.push(x.clone());
        }
    }
    Some(Value::Array(out.into_iter().map(Value::String).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn different_fields_merge() {
        let base = json!({"label": "web", "port": 22, "notes": ""});
        let ours = json!({"label": "web-1", "port": 22, "notes": ""});
        let theirs = json!({"label": "web", "port": 2222, "notes": ""});
        assert_eq!(
            merge3(&base, &ours, &theirs).unwrap(),
            json!({"label": "web-1", "port": 2222, "notes": ""})
        );
    }

    #[test]
    fn same_field_different_values_conflict() {
        let base = json!({"auth": {"type": "password", "password": "a"}});
        let ours = json!({"auth": {"type": "password", "password": "b"}});
        let theirs = json!({"auth": {"type": "password", "password": "c"}});
        assert_eq!(
            merge3(&base, &ours, &theirs).unwrap_err(),
            vec!["auth".to_string()]
        );
    }

    #[test]
    fn added_and_removed_fields() {
        let base = json!({"a": 1});
        let ours = json!({"a": 1, "b": 2});
        let theirs = json!({});
        // We added b; they removed a: both apply.
        assert_eq!(merge3(&base, &ours, &theirs).unwrap(), json!({"b": 2}));
    }

    #[test]
    fn tags_merge_as_sets() {
        let base = json!({"tags": ["db", "eu"]});
        let ours = json!({"tags": ["db", "eu", "primary"]});
        let theirs = json!({"tags": ["db", "critical"]}); // removed eu, added critical
        assert_eq!(
            merge3(&base, &ours, &theirs).unwrap(),
            json!({"tags": ["db", "primary", "critical"]})
        );
    }

    #[test]
    fn non_objects_are_atomic() {
        assert!(merge3(&json!("a"), &json!("b"), &json!("c")).is_err());
        assert_eq!(
            merge3(&json!("a"), &json!("a"), &json!("c")).unwrap(),
            json!("c")
        );
    }
}
