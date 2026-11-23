//! Tests of saving `spaceTemplates` (a child of `settings`, which they use).

use super::*;

#[test]
fn save_space_templates_writes_only_that_key_and_keeps_the_rest() {
    let home = std::env::temp_dir().join(format!("poimen-tpls-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(config_path(&home).parent().unwrap()).unwrap();
    std::fs::write(
        config_path(&home),
        "{\n  \"theme\": \"dark\",\n  \"spaceTemplates\": []\n}\n",
    )
    .unwrap();
    let templates = serde_json::json!([
        {"name": "work", "match": {"dir": "~/src", "remote": {"provider": "github", "owner": "acme"}},
         "agent": "claude", "model": "opus",
         "layout": {"panes": [{"name": "main", "command": "ls"}]}},
        {"name": "az", "match": {"remote": {"provider": "azure-devops", "organization": "o", "project": "p"}},
         "agent": "copilot"}
    ]);
    save_space_templates(&home, &templates).unwrap();
    let text = std::fs::read_to_string(config_path(&home)).unwrap();
    assert!(text.contains("\"theme\": \"dark\""));
    let s = load(&home);
    assert!(s.warnings.is_empty(), "{:?}", s.warnings);
    assert_eq!(s.theme, "dark");
    assert_eq!(s.space_templates.len(), 2);
    assert_eq!(s.space_templates[0].model.as_deref(), Some("opus"));
    save_space_templates(&home, &serde_json::json!([])).unwrap();
    assert!(load(&home).space_templates.is_empty());
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn saving_templates_keeps_a_layout_name_reference_as_a_string() {
    let t = serde_json::json!([{"name": "a", "agent": "claude", "layout": "review"}]);
    let out = set_space_templates_text("{}", &t).unwrap();
    let back: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(back["spaceTemplates"][0]["layout"], "review");
    let bad = serde_json::json!([{"name": "a", "agent": "claude", "layout": 3}]);
    assert!(set_space_templates_text("{}", &bad).is_err());
}

#[test]
fn save_space_templates_creates_the_file_and_rejects_invalid_input() {
    let ok = serde_json::json!([{"name": "a", "match": {"dir": "~/a"}, "agent": "claude"}]);
    assert!(
        set_space_templates_text("{}", &ok)
            .unwrap()
            .contains("\"spaceTemplates\"")
    );
    for bad in [
        serde_json::json!([
            {"name": "a", "agent": "claude"},
            {"name": "b", "match": {}, "agent": "claude"}
        ]),
        serde_json::json!([{"name": "a", "match": {"dir": "rel"}, "agent": "claude"}]),
        serde_json::json!([{"name": "a", "match": {"dir": "~/a"}}]),
        serde_json::json!([
            {"name": "a", "match": {"dir": "~/a"}, "agent": "claude"},
            {"name": "a", "match": {"dir": "~/b"}, "agent": "claude"}
        ]),
        serde_json::json!({"name": "a"}),
    ] {
        let err = set_space_templates_text("{}", &bad).unwrap_err();
        assert!(err.starts_with("spaceTemplates: "), "{err}");
    }
}
