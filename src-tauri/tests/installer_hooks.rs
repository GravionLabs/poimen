//! The NSIS hooks that edit the user PATH (#519, #671): native NSIS registry commands only, because an
//! installer that starts PowerShell to write the registry trips Microsoft Defender heuristics.

use std::fs;
use std::path::PathBuf;

fn hooks() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("windows/nsis-hooks.nsh");
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The body of one macro, from `!macro <name>` to its `!macroend`.
fn macro_body(text: &str, name: &str) -> String {
    let start = text
        .find(&format!("!macro {name}"))
        .unwrap_or_else(|| panic!("macro {name} missing"));
    let rest = &text[start..];
    let end = rest.find("!macroend").expect("!macroend missing");
    rest[..end].to_string()
}

/// Lines that are code: no comments.
fn code(text: &str) -> String {
    text.lines()
        .filter(|l| !l.trim_start().starts_with(';'))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn hooks_start_no_child_process() {
    let code = code(&hooks()).to_lowercase();
    for banned in ["powershell", "pwsh", "nsexec::", "exec "] {
        assert!(!code.contains(banned), "`{banned}` found in the NSIS hooks");
    }
}

#[test]
fn install_adds_the_install_dir_once_as_expand_string() {
    let body = macro_body(&hooks(), "NSIS_HOOK_POSTINSTALL");
    assert!(body.contains(r#"ReadRegStr $R0 HKCU "Environment" "Path""#));
    assert!(body.contains(r#"WriteRegExpandStr HKCU "Environment" "Path""#));
    assert!(!body.contains("WriteRegStr"), "REG_EXPAND_SZ must be kept");
    // An existing entry jumps over the write.
    assert!(body.contains(r#"StrCmp $R4 "$INSTDIR" poimen_path_add_done"#));
    let compare = body.find("poimen_path_add_done\n").unwrap();
    let write = body.find("WriteRegExpandStr").unwrap();
    assert!(
        compare < write,
        "the duplicate check comes before the write"
    );
}

#[test]
fn install_handles_empty_path_and_trailing_semicolon() {
    let body = macro_body(&hooks(), "NSIS_HOOK_POSTINSTALL");
    assert!(body.contains(r#"StrCpy $R5 "$INSTDIR""#), "empty Path");
    assert!(body.contains(r#"StrCpy $R5 "$R0$INSTDIR""#), "trailing ;");
    assert!(body.contains(r#"StrCpy $R5 "$R0;$INSTDIR""#), "append");
}

#[test]
fn uninstall_removes_only_the_install_dir_and_keeps_the_order() {
    let body = macro_body(&hooks(), "NSIS_HOOK_POSTUNINSTALL");
    assert!(body.contains(r#"StrCmp $R4 "$INSTDIR" poimen_path_remove_next"#));
    assert!(
        body.contains(r#"StrCpy $R5 "$R5;$R4""#),
        "others appended in order"
    );
    assert!(body.contains(r#"WriteRegExpandStr HKCU "Environment" "Path" "$R5""#));
    assert!(!body.contains("WriteRegStr"));
    // A missing Path value is left alone.
    assert!(body.contains(r#"StrCmp $R0 "" poimen_path_remove_done"#));
}

#[test]
fn both_hooks_broadcast_the_environment_change() {
    let text = hooks();
    for name in ["NSIS_HOOK_POSTINSTALL", "NSIS_HOOK_POSTUNINSTALL"] {
        assert!(
            macro_body(&text, name).contains(
                r#"SendMessage ${HWND_BROADCAST} ${WM_WININICHANGE} 0 "STR:Environment""#
            ),
            "{name} must broadcast WM_SETTINGCHANGE"
        );
    }
}

#[test]
fn labels_are_unique_across_the_hooks() {
    let text = hooks();
    let mut labels: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("poimen_path_") && l.ends_with(':'))
        .collect();
    let n = labels.len();
    labels.sort();
    labels.dedup();
    assert_eq!(n, labels.len(), "duplicate NSIS label");
    assert!(n > 0);
}
