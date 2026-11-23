//! Tests of the `sounds` setting and `sound_file` (a child of `settings`, which they use).

use super::*;

fn rows(json: &str) -> Settings {
    resolve(Some(json), || None)
}

#[test]
fn sounds_default_to_off_and_parse_true_false_and_paths() {
    assert_eq!(rows("{}").sounds, Sounds::default());
    let s = rows(r#"{"sounds":{"blocked":true,"done":"~/ding.wav"}}"#);
    assert_eq!(s.sounds.blocked, SoundSetting::Builtin);
    assert_eq!(s.sounds.done, SoundSetting::File("~/ding.wav".into()));
    assert!(s.warnings.is_empty(), "{:?}", s.warnings);
    let json = serde_json::to_value(&s.sounds).unwrap();
    assert_eq!(
        json,
        serde_json::json!({"blocked": true, "done": "~/ding.wav"})
    );
}

#[test]
fn other_sound_values_warn_and_stay_off() {
    let s = rows(r#"{"sounds":{"blocked":3,"done":""}}"#);
    assert_eq!(s.sounds, Sounds::default());
    assert_eq!(s.warnings.len(), 2, "{:?}", s.warnings);
    assert_eq!(rows(r#"{"sounds":true}"#).warnings.len(), 1);
}

#[test]
fn set_sound_text_writes_one_key_and_refuses_other_values() {
    let out = set_sound_text("{\"theme\":\"dark\"}", "done", &Value::Bool(true)).unwrap();
    assert!(
        out.contains("\"done\": true") || out.contains("\"done\":true"),
        "{out}"
    );
    assert!(set_sound_text("{}", "other", &Value::Bool(true)).is_err());
    assert!(set_sound_text("{}", "done", &Value::from(3)).is_err());
}

fn sound_home(name: &str, config: &str) -> PathBuf {
    let home = std::env::temp_dir().join(format!("poimen-sound-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(config_path(&home).parent().unwrap()).unwrap();
    std::fs::write(config_path(&home), config).unwrap();
    home
}

#[test]
fn sound_file_reads_the_configured_file() {
    let home = sound_home("ok", r#"{"sounds":{"done":"~/ding.wav"}}"#);
    std::fs::write(home.join("ding.wav"), [1u8, 2, 3]).unwrap();
    assert_eq!(read_sound_file(&home, "done").unwrap(), vec![1, 2, 3]);
    // `blocked` is off: the caller cannot name a path, only a kind.
    assert!(read_sound_file(&home, "blocked").is_err());
    assert!(read_sound_file(&home, "/etc/passwd").is_err());
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn sound_file_refuses_missing_large_and_directory() {
    let home = sound_home(
        "bad",
        r#"{"sounds":{"blocked":"~/none.wav","done":"~/dir"}}"#,
    );
    assert!(read_sound_file(&home, "blocked").is_err());
    std::fs::create_dir(home.join("dir")).unwrap();
    assert!(
        read_sound_file(&home, "done")
            .unwrap_err()
            .contains("regular")
    );
    std::fs::write(
        home.join("none.wav"),
        vec![0u8; MAX_SOUND_BYTES as usize + 1],
    )
    .unwrap();
    assert!(
        read_sound_file(&home, "blocked")
            .unwrap_err()
            .contains("1 MiB")
    );
    std::fs::write(home.join("none.wav"), vec![0u8; MAX_SOUND_BYTES as usize]).unwrap();
    assert!(read_sound_file(&home, "blocked").is_ok());
    let _ = std::fs::remove_dir_all(&home);
}
