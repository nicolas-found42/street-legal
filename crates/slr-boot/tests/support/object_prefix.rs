//! Compare the observed original allocation prefix, including String contents.
use std::path::Path;

/// Normalize only the terminal hexadecimal heap identity printed by toString.
/// The class/descriptor spelling remains significant.
fn string_projection(text: &str) -> String {
    if let Some((prefix, suffix)) = text.rsplit_once('<') {
        if let Some(word) = suffix.strip_suffix('>') {
            if !word.is_empty() && word.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return format!("{prefix}<reference>");
            }
        }
    }
    text.to_owned()
}

pub fn assert_original_object_prefix(repository: &Path, output: &Path) {
    let original = repository
        .join("re/out/headless/finalizer-investigation/callers-observed/original-facts.json");
    let original: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(original)
            .expect("generate the original finalizer allocation capture first"),
    )
    .unwrap();
    let history = &original["finalizer_history"];
    assert_eq!(history["truncated"], false);
    assert_eq!(history["strings_pending"], 0);
    let original = history["allocations"].as_array().unwrap();
    let port = std::fs::read_to_string(output.join("objects-before-frames.tsv"))
        .expect("run the stock boot before comparing its allocation prefix");
    let port: Vec<_> = port.lines().collect();
    assert_eq!(port.len(), original.len(), "allocation count");
    for (index, (original, port)) in original.iter().zip(port).enumerate() {
        let fields: Vec<_> = port.splitn(3, '\t').collect();
        assert_eq!(
            fields.len(),
            3,
            "allocation row {} needs String contents",
            index + 1
        );
        assert_eq!(fields[0].parse::<usize>().unwrap(), index + 1);
        assert_eq!(original["ordinal"].as_u64().unwrap(), (index + 1) as u64);
        assert_eq!(
            fields[1],
            original["class"].as_str().unwrap(),
            "allocation {} class",
            index + 1
        );
        let port_text = fields[2]
            .strip_prefix("Some(")
            .and_then(|text| text.strip_suffix(')'));
        if let Some(expected) = original["string_text"].as_str() {
            let port_text: String =
                serde_json::from_str(port_text.expect("String contents missing")).unwrap();
            assert_eq!(
                string_projection(&port_text),
                string_projection(expected),
                "allocation {} String bytes",
                index + 1
            );
        } else {
            assert_eq!(
                fields[2],
                "None",
                "allocation {} unexpectedly String",
                index + 1
            );
        }
    }
}
