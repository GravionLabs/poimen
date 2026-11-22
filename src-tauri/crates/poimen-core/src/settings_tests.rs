#[test]
fn silent_after_defaults_parses_and_warns() {
    assert_eq!(silent_after_text("{}"), 10);
    assert_eq!(silent_after_text("not json"), 10);
    assert_eq!(silent_after_text(r#"{"agents":{"silentAfter":3}}"#), 3);
    assert_eq!(silent_after_text(r#"{"agents":{"silentAfter":0}}"#), 0);
    let mut w = Vec::new();
    for bad in [r#""x""#, "-1", "1.5", "true"] {
        let v: Value = serde_json::from_str(&format!(r#"{{"silentAfter":{bad}}}"#)).unwrap();
        assert_eq!(resolve_silent_after(&v, &mut w), 10, "{bad}");
    }
    assert_eq!(w.len(), 4);
}

#[test]
fn hibernate_after_is_minutes_and_off_by_default() {
    use super::hibernate_after_minutes as h;
    let v = |t: &str| serde_json::from_str::<serde_json::Value>(t).unwrap();
    assert_eq!(h(&v("{}")), Ok(None));
    assert_eq!(h(&v(r#"{"agents":{}}"#)), Ok(None));
    assert_eq!(h(&v(r#"{"agents":{"hibernateAfter":0}}"#)), Ok(None));
    assert_eq!(h(&v(r#"{"agents":{"hibernateAfter":30}}"#)), Ok(Some(30)));
    assert!(h(&v(r#"{"agents":{"hibernateAfter":-1}}"#)).is_err());
    assert!(h(&v(r#"{"agents":{"hibernateAfter":"5"}}"#)).is_err());
    assert!(h(&v(r#"{"agents":{"hibernateAfter":1.5}}"#)).is_err());
}

use super::*;

#[test]
fn set_layouts_validates_and_keeps_the_rest() {
    let text = "{\n  // mine\n  \"theme\": \"dark\"\n}";
    let layouts = serde_json::json!([{"name": "review", "tabs": [
        {"name": "agent", "command": "claude", "children": [
            {"name": "tests", "place": {"direction": "right", "ratio": 0.4}}]}]}]);
    let out = set_layouts_text(text, &layouts).unwrap();
    assert!(out.contains("// mine") && out.contains("\"theme\": \"dark\""));
    let back: Value = serde_json::from_str(&out.replace("  // mine\n", "")).unwrap();
    assert_eq!(back["layouts"][0]["name"], "review");
    for bad in [
        serde_json::json!({"name": "a"}),
        serde_json::json!([{"name": "a", "tabs": []}]),
        serde_json::json!([{"name": "a", "tabs": [{"name": "x"}]}, {"name": "a", "tabs": [{"name": "y"}]}]),
        serde_json::json!([{"name": "a", "tabs": [{"name": "x", "place": {"direction": "left"}}]}]),
    ] {
        let err = set_layouts_text("{}", &bad).unwrap_err();
        assert!(err.starts_with("layouts: "), "{err}");
    }
}

#[test]
fn set_layouts_refuses_to_drop_a_layout_a_template_uses() {
    let text = r#"{"layouts": [{"name": "review", "tabs": [{"name": "a"}]},
        {"name": "other", "tabs": [{"name": "b"}]}],
      "spaceTemplates": [{"name": "t1", "agent": "claude", "layout": "review"},
        {"name": "t2", "agent": "claude", "layout": "review"}]}"#;
    let only_other = serde_json::json!([{"name": "other", "tabs": [{"name": "b"}]}]);
    let err = set_layouts_text(text, &only_other).unwrap_err();
    assert!(
        err.contains("\"review\"") && err.contains("t1, t2"),
        "{err}"
    );
    // A rename is a drop of the old name.
    let renamed = serde_json::json!([{"name": "look", "tabs": [{"name": "a"}]},
        {"name": "other", "tabs": [{"name": "b"}]}]);
    assert!(set_layouts_text(text, &renamed).is_err());
    // Deleting an unused one is fine, and so is editing a used one in place.
    let edited = serde_json::json!([{"name": "review", "tabs": [{"name": "z"}]}]);
    assert!(set_layouts_text(text, &edited).is_ok());
}

#[test]
fn set_layouts_never_drops_entries_it_cannot_read() {
    let text =
        r#"{"layouts": [{"name": "a", "tabs": [{"name": "x"}]}, {"name": "bad", "tabs": []}]}"#;
    let new = serde_json::json!([{"name": "a", "tabs": [{"name": "y"}]}]);
    let err = set_layouts_text(text, &new).unwrap_err();
    assert!(err.contains("cannot read"), "{err}");
}

#[test]
fn set_layouts_leaves_an_unchanged_list_alone() {
    let layouts = serde_json::json!([{"name": "a", "tabs": [{"name": "x"}]}]);
    let text = format!("{{ /* keep */ \"layouts\": {}  }}", layouts);
    assert_eq!(set_layouts_text(&text, &layouts).unwrap(), text);
}

#[test]
fn save_with_does_not_rewrite_an_unchanged_file() {
    let dir = std::env::temp_dir().join(format!("poimen-nowrite-{}", std::process::id()));
    let path = config_path(&dir);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "{ }").unwrap();
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(old)
        .unwrap();
    let before = std::fs::metadata(&path).unwrap().modified().unwrap();
    save_with(&dir, |t| Ok(t.to_owned())).unwrap();
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        before
    );
    let _ = std::fs::remove_dir_all(&dir);
}

fn removed(text: &str, path: &[&str]) -> String {
    remove_path_text(text, path).unwrap()
}

#[test]
fn remove_path_drops_a_middle_entry_and_keeps_its_comment() {
    let text = "{\n  \"a\": 1,\n  // about b\n  \"b\": 2,\n  \"c\": 3\n}";
    assert_eq!(
        removed(text, &["b"]),
        "{\n  \"a\": 1,\n  // about b\n  \"c\": 3\n}"
    );
}

#[test]
fn remove_path_drops_the_last_entry_and_the_comma_before_its_comment() {
    let text = "{\n  \"a\": 1,\n  // about b\n  \"b\": 2\n}";
    assert_eq!(removed(text, &["b"]), "{\n  \"a\": 1\n  // about b\n}");
    let block = "{ \"a\": [1, 2], /* b, c */ \"b\": { \"x\": \"y,\" } }";
    assert_eq!(removed(block, &["b"]), "{ \"a\": [1, 2] /* b, c */ }");
    let commas = "{ \"a\": \"1,2\", // x, y\n \"b\": 2 }";
    assert_eq!(removed(commas, &["b"]), "{ \"a\": \"1,2\" // x, y\n  }");
    let middle = "{ \"a\": 1, // x\n \"b\": 2, \"c\": 3 }";
    assert_eq!(removed(middle, &["b"]), "{ \"a\": 1, // x\n  \"c\": 3 }");
}

#[test]
fn remove_path_drops_a_last_nested_entry_next_to_comments() {
    let text = r#"{
  "trustedProjects": { "/r": "h" },
  "sidebar": {
// about layout
"layout": "tree",
// about agents
"agents": "list"
  }
}"#;
    let next = removed(text, &["sidebar", "agents"]);
    assert_eq!(
        next,
        r#"{
  "trustedProjects": { "/r": "h" },
  "sidebar": {
// about layout
"layout": "tree"
// about agents
  }
}"#
    );
    assert_eq!(
        removed(&next, &["sidebar", "layout"]),
        r#"{
  "trustedProjects": { "/r": "h" },
  "sidebar": {
// about layout
// about agents
  }
}"#
    );
}

#[test]
fn remove_path_of_the_only_entry_or_a_missing_key() {
    assert_eq!(removed("{ // only\n \"a\": 1 }", &["a"]), "{ // only\n  }");
    assert_eq!(removed("{ \"a\": 1 }", &["b"]), "{ \"a\": 1 }");
    assert_eq!(removed("{ \"a\": 1 }", &["b", "c"]), "{ \"a\": 1 }");
}

#[test]
fn remove_path_errors_on_a_non_object_text_or_parent() {
    assert!(remove_path_text("[]", &["a"]).is_err());
    assert!(remove_path_text("{ \"a\": 1 }", &[]).is_err());
    assert_eq!(
        remove_path_text("{ \"a\": 1 }", &["a", "c"]),
        Err("a in config.json is not an object".into())
    );
}

#[test]
fn group_worktrees_defaults_to_true_and_warns_on_a_wrong_type() {
    let group = |json: &str| {
        let v: Value = serde_json::from_str(json).unwrap();
        let mut w = Vec::new();
        (resolve_group_worktrees(&v, &mut w), w)
    };
    assert_eq!(group("{}"), (true, vec![]));
    assert_eq!(group(r#"{"groupWorktrees":false}"#), (false, vec![]));
    let (g, w) = group(r#"{"groupWorktrees":"no"}"#);
    assert!(g && w[0].contains("sidebar.groupWorktrees"));
}

#[test]
fn sidebar_sort_parses_and_an_unknown_value_warns_as_manual() {
    let sort = |json: &str| {
        let mut w = Vec::new();
        let v: Value = serde_json::from_str(json).unwrap();
        (resolve_sidebar_sort(&v, &mut w), w)
    };
    assert_eq!(sort("{}"), (SidebarSort::Manual, vec![]));
    assert_eq!(sort(r#"{"sort":"name"}"#).0, SidebarSort::Name);
    assert_eq!(sort(r#"{"sort":"manual"}"#).0, SidebarSort::Manual);
    assert_eq!(sort(r#"{"sort":"attention"}"#).0, SidebarSort::Attention);
    assert_eq!(sort(r#"{"sort":"recent"}"#).0, SidebarSort::Recent);
    let (s, w) = sort(r#"{"sort":"newest"}"#);
    assert_eq!(s, SidebarSort::Manual);
    assert!(w[0].contains("sidebar.sort"));
    assert_eq!(sort(r#"{"sort":3}"#).0, SidebarSort::Manual);
    assert_eq!(SidebarSort::Name.as_str(), "name");
}

#[test]
fn the_rc_channel_has_its_own_config_dir() {
    let home = Path::new("/home/jo");
    assert_eq!(
        config_dir_for(home, poimen_protocol::Channel::Stable),
        PathBuf::from("/home/jo/.config/poimen")
    );
    assert_eq!(
        config_dir_for(home, poimen_protocol::Channel::Rc),
        PathBuf::from("/home/jo/.config/poimen-rc")
    );
    assert_eq!(config_path(home), config_dir(home).join("config.json"));
}

#[test]
fn set_trusted_projects_keeps_comments_and_other_keys() {
    let text = "{\n  // keep\n  \"theme\": \"dark\"\n}";
    let map = BTreeMap::from([("C:\\work\\app".to_owned(), "h1".to_owned())]);
    let out = set_trusted_projects_text(text, &map).unwrap();
    assert!(out.contains("// keep") && out.contains("\"theme\": \"dark\""));
    let parsed: Value = serde_json::from_str(&out.replace("  // keep\n", "")).unwrap();
    assert_eq!(parsed["trustedProjects"]["C:\\work\\app"], "h1");
    // Replacing an existing entry set keeps the rest too.
    let again = set_trusted_projects_text(&out, &BTreeMap::new()).unwrap();
    assert!(again.contains("// keep") && again.contains("\"trustedProjects\": {}"));
}

#[test]
fn trusted_projects_skip_bad_entries_with_warnings() {
    let mut warnings = Vec::new();
    let value: Value = serde_json::from_str(r#"{"/a":"abc","/b":3,"/c":""}"#).unwrap();
    let map = parse_trusted_projects(&value, &mut warnings);
    assert_eq!(map.keys().collect::<Vec<_>>(), ["/a"]);
    assert_eq!(warnings.len(), 2);
    let none = parse_trusted_projects(&serde_json::json!([1]), &mut warnings);
    assert!(none.is_empty());
    assert_eq!(warnings.len(), 3);
}

#[test]
fn trust_and_revoke_write_only_trusted_projects() {
    let home = std::env::temp_dir().join(format!("poimen-core-trust-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    assert!(trusted_projects(&home).is_empty());
    revoke_project(&home, "/a").unwrap();
    assert!(!config_path(&home).exists());
    trust_project(&home, "/a", "h1").unwrap();
    trust_project(&home, "/a", "h2").unwrap();
    assert_eq!(trusted_projects(&home)["/a"], "h2");
    let text = std::fs::read_to_string(config_path(&home)).unwrap();
    assert!(text.contains("\"trustedProjects\""));
    revoke_project(&home, "/a").unwrap();
    assert!(trusted_projects(&home).is_empty());
    let _ = std::fs::remove_dir_all(&home);
}

fn temp_home(name: &str) -> PathBuf {
    let home = std::env::temp_dir().join(format!("poimen-core-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(config_dir(&home)).unwrap();
    home
}

#[test]
fn trust_keeps_comments_other_keys_and_entries_in_a_commented_file() {
    let home = temp_home("trust-comments");
    let text = "{\n  // my settings\n  \"theme\": \"dark\", /* inline */\n  \"trustedProjects\": {\"/old\": \"h0\"} // trailing\n}\n";
    std::fs::write(config_path(&home), text).unwrap();
    trust_project(&home, "/new", "h1").unwrap();
    let out = std::fs::read_to_string(config_path(&home)).unwrap();
    for kept in [
        "// my settings",
        "\"theme\": \"dark\"",
        "/* inline */",
        "// trailing",
    ] {
        assert!(out.contains(kept), "{kept} lost: {out}");
    }
    let map = trusted_projects_text(&out).unwrap();
    assert_eq!(map.get("/old").map(String::as_str), Some("h0"));
    assert_eq!(map.get("/new").map(String::as_str), Some("h1"));
    revoke_project(&home, "/old").unwrap();
    let out = std::fs::read_to_string(config_path(&home)).unwrap();
    assert!(out.contains("// my settings") && out.contains("// trailing"));
    assert_eq!(trusted_projects_text(&out).unwrap().len(), 1);
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn a_trusted_projects_value_that_cannot_be_read_is_never_overwritten() {
    let home = temp_home("trust-broken");
    let text = "{\"trustedProjects\": {\"/a\": \"h\", // note\n}}";
    std::fs::write(config_path(&home), text).unwrap();
    let err = trust_project(&home, "/b", "h2").unwrap_err();
    assert!(err.contains("not changed"), "{err}");
    assert_eq!(std::fs::read_to_string(config_path(&home)).unwrap(), text);
    std::fs::write(config_path(&home), "{\"trustedProjects\": [1]}").unwrap();
    assert!(trust_project(&home, "/b", "h2").is_err());
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn two_writers_in_turn_keep_both_changes() {
    // The app writes its keys (`theme`, `sidebar.layout`) and the server `trustedProjects`,
    // each reading the file right before the write.
    let home = temp_home("two-writers");
    std::fs::write(
        config_path(&home),
        "{\n  // mine\n  \"theme\": \"light\"\n}\n",
    )
    .unwrap();
    trust_project(&home, "/a", "h1").unwrap();
    save_with(&home, |t| set_key_text(t, "theme", "\"dark\"")).unwrap();
    trust_project(&home, "/b", "h2").unwrap();
    save_with(&home, |t| {
        set_path_text(t, &["sidebar", "layout"], "\"split\"")
    })
    .unwrap();
    let out = std::fs::read_to_string(config_path(&home)).unwrap();
    assert!(out.contains("// mine"), "{out}");
    let parsed: Value = serde_json::from_str(&out.replace("  // mine\n", "")).unwrap();
    assert_eq!(parsed["theme"], "dark");
    assert_eq!(parsed["sidebar"]["layout"], "split");
    assert_eq!(parsed["trustedProjects"]["/a"], "h1");
    assert_eq!(parsed["trustedProjects"]["/b"], "h2");
    let leftovers: Vec<_> = std::fs::read_dir(config_dir(&home))
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty());
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn tasks_and_templates_parsers_warn_and_skip_invalid_entries() {
    let mut warnings = Vec::new();
    let tasks = parse_tasks(
        &serde_json::json!({"worktreeDir": "../{repo}-{nr}", "modes": [{"name": "a"}, {"name": "a"}, 3]}),
        "using the default agent mode",
        &mut warnings,
    );
    assert_eq!(tasks.worktree_dir.as_deref(), Some("../{repo}-{nr}"));
    assert_eq!(tasks.modes.unwrap().len(), 1);
    assert_eq!(warnings.len(), 2);
    let mut warnings = Vec::new();
    let templates = parse_space_templates(
        &serde_json::json!([{"name": "t", "agent": "claude"}, {"agent": "x"}]),
        &mut warnings,
    );
    assert_eq!(templates.len(), 1);
    assert_eq!(warnings.len(), 1);
}
