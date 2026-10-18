use clap::CommandFactory;
use mycqu_cli::cli::Cli;
use mycqu_cli::output::{ApiResponse, EXIT_INCOMPLETE, EXIT_INVALID_ARGUMENT};
use mycqu_cli::schema::{parse_catalog, schema_catalog};

#[test]
fn test_schema_catalog_structure() {
    let catalog = schema_catalog().expect("catalog must parse");
    assert!(catalog.is_object());
    let commands = catalog["commands"].as_array().expect("commands array");
    assert!(
        commands.len() >= 15,
        "catalog must contain core campus commands"
    );
    for cmd in commands {
        assert!(cmd["path"].is_string());
        assert!(cmd["description"].is_string());
        assert!(cmd["requires_auth"].is_boolean());
        assert!(cmd["has_side_effects"].is_boolean());
        assert!(cmd["possible_errors"].is_array());
        assert!(
            cmd["input_schema"].is_object(),
            "{} missing input_schema",
            cmd["path"]
        );
        assert!(
            cmd["input_schema"]["properties"].is_object(),
            "{} input_schema lacks properties",
            cmd["path"]
        );
        assert!(
            cmd["input_schema"]["required"].is_array(),
            "{} input_schema lacks required list",
            cmd["path"]
        );
        assert!(
            cmd["output_schema"].is_object(),
            "{} missing output_schema",
            cmd["path"]
        );
        // 参数元数据只保留 input_schema，不再维护重复的 options 清单
        assert!(
            cmd.get("options").is_none(),
            "{} must not duplicate parameter metadata in options",
            cmd["path"]
        );
        if cmd["has_side_effects"].as_bool() == Some(true) {
            assert!(
                cmd["confirmation"].is_object(),
                "{} has side effects but no confirmation metadata",
                cmd["path"]
            );
        }
    }
}

#[test]
fn test_schema_catalog_paths_unique() {
    let catalog = schema_catalog().expect("catalog must parse");
    let commands = catalog["commands"].as_array().expect("commands array");
    let paths: Vec<&str> = commands
        .iter()
        .map(|c| c["path"].as_str().expect("path string"))
        .collect();
    let unique: std::collections::HashSet<&&str> = paths.iter().collect();
    assert_eq!(unique.len(), paths.len(), "catalog paths must be unique");
}

#[test]
fn test_parse_catalog_rejects_malformed_input() {
    assert!(parse_catalog("not json").is_err());
    assert!(parse_catalog("{}").is_err(), "catalog must be an array");
    assert!(parse_catalog("[]").is_ok());
}

/// 收集叶子命令及其 clap 定义，用于比对命令目录。
fn leaf_commands<'cmd>(
    cmd: &'cmd clap::Command,
    prefix: &str,
    out: &mut Vec<(String, &'cmd clap::Command)>,
) {
    for sub in cmd.get_subcommands() {
        let name = sub.get_name().to_string();
        let path = if prefix.is_empty() {
            name
        } else {
            format!("{prefix} {name}")
        };
        if sub.has_subcommands() {
            leaf_commands(sub, &path, out);
        } else {
            out.push((path, sub));
        }
    }
}

/// clap 参数类型到 JSON Schema 类型的映射；未知类型返回 `None` 表示跳过。
fn json_type(arg: &clap::Arg) -> Option<&'static str> {
    use std::any::TypeId;

    if matches!(arg.get_action(), clap::ArgAction::SetTrue) {
        return Some("boolean");
    }
    let id = arg.get_value_parser().type_id();
    if id == TypeId::of::<bool>() {
        Some("boolean")
    } else if id == TypeId::of::<u8>()
        || id == TypeId::of::<u16>()
        || id == TypeId::of::<u32>()
        || id == TypeId::of::<u64>()
        || id == TypeId::of::<usize>()
    {
        Some("integer")
    } else if id == TypeId::of::<String>() {
        Some("string")
    } else if id == TypeId::of::<Vec<String>>() {
        Some("array")
    } else {
        None
    }
}

#[test]
fn test_catalog_matches_clap_definitions() {
    let cli_command = Cli::command();
    let mut leaves = Vec::new();
    leaf_commands(&cli_command, "", &mut leaves);
    // schema 是能力发现机制本身，不属于被发现的命令目录
    leaves.retain(|(path, _)| path != "schema");

    let catalog = schema_catalog().expect("catalog must parse");
    let commands = catalog["commands"].as_array().expect("commands array");

    let mut cli_paths: Vec<String> = leaves.iter().map(|(path, _)| path.clone()).collect();
    cli_paths.sort();
    let mut catalog_paths: Vec<String> = commands
        .iter()
        .map(|c| c["path"].as_str().expect("path string").to_string())
        .collect();
    catalog_paths.sort();
    assert_eq!(
        cli_paths, catalog_paths,
        "clap command tree and catalog.json drifted apart"
    );

    for (path, cli_cmd) in &leaves {
        let entry = commands
            .iter()
            .find(|c| c["path"].as_str() == Some(path.as_str()))
            .unwrap_or_else(|| panic!("{path} missing in catalog"));
        let schema = &entry["input_schema"];
        let properties = schema["properties"]
            .as_object()
            .unwrap_or_else(|| panic!("{path}: input_schema.properties missing"));
        let schema_required: Vec<String> = schema["required"]
            .as_array()
            .unwrap_or_else(|| panic!("{path}: input_schema.required missing"))
            .iter()
            .map(|v| {
                v.as_str()
                    .expect("required entries are strings")
                    .to_string()
            })
            .collect();

        let mut cli_names = Vec::new();
        let mut cli_required = Vec::new();

        for arg in cli_cmd.get_arguments() {
            let id = arg.get_id().as_str();
            if id == "help" || id == "version" {
                continue;
            }
            let Some(long) = arg.get_long() else {
                // 目录只描述具名参数；位置参数仅出现在 schema 命令上
                panic!("{path}: unexpected positional argument '{id}'");
            };
            let name = format!("--{long}");
            let property = properties
                .get(&name)
                .unwrap_or_else(|| panic!("{path}: schema is missing {name}"));

            let expected_type = json_type(arg);
            if let Some(expected) = expected_type {
                assert_eq!(
                    property["type"].as_str(),
                    Some(expected),
                    "{path} {name}: JSON type mismatch"
                );
            }

            if let Some(default) = arg.get_default_values().first() {
                let schema_default = &property["default"];
                let actual = match schema_default {
                    serde_json::Value::String(value) => value.clone(),
                    serde_json::Value::Null => {
                        panic!("{path} {name}: clap declares a default but schema does not")
                    }
                    other => other.to_string(),
                };
                assert_eq!(
                    actual,
                    default.to_string_lossy(),
                    "{path} {name}: default value mismatch"
                );
            }

            let mut possible: Vec<String> = arg
                .get_possible_values()
                .iter()
                .map(|value| value.get_name().to_string())
                .collect();
            // 布尔参数的 true/false 是隐式取值，不需要在 Schema 中声明为枚举
            if expected_type == Some("boolean") {
                possible.clear();
            }
            if !possible.is_empty() {
                let mut schema_enum: Vec<String> = property["enum"]
                    .as_array()
                    .unwrap_or_else(|| panic!("{path} {name}: schema is missing enum values"))
                    .iter()
                    .map(|v| v.as_str().expect("enum entries are strings").to_string())
                    .collect();
                possible.sort();
                schema_enum.sort();
                assert_eq!(schema_enum, possible, "{path} {name}: enum mismatch");
            }

            cli_names.push(name.clone());
            if arg.is_required_set() {
                cli_required.push(name);
            }
        }

        cli_names.sort();
        let mut schema_names: Vec<String> = properties.keys().cloned().collect();
        schema_names.sort();
        assert_eq!(cli_names, schema_names, "{path}: flag set mismatch");

        cli_required.sort();
        let mut required_sorted = schema_required;
        required_sorted.sort();
        assert_eq!(
            cli_required, required_sorted,
            "{path}: required flags mismatch"
        );
    }
}

#[test]
fn test_api_response_envelope() {
    let mut resp = ApiResponse::success("test_data", false, Some(1));
    resp.meta.stop_reason = Some("Reached page limit".to_string());
    assert!(resp.ok);
    assert_eq!(resp.schema_version, 1);
    assert_eq!(resp.data, Some("test_data"));
    assert_eq!(resp.meta.timezone, "Asia/Shanghai");
    assert!(!resp.meta.complete);
    assert_eq!(resp.meta.count, Some(1));
    assert_eq!(resp.meta.stop_reason.as_deref(), Some("Reached page limit"));

    let err_resp = ApiResponse::<()>::err("INVALID_ARGUMENT", "test error", false, None, None);
    assert!(!err_resp.ok);
    assert_eq!(err_resp.error.unwrap().code, "INVALID_ARGUMENT");
    assert_eq!(EXIT_INVALID_ARGUMENT, 2);
    assert_eq!(EXIT_INCOMPLETE, 7);
}
#[cfg(windows)]
mod store_tests {
    use mycqu_cli::challenge::{ChallengeCheck, StoredChallenge, check_challenge};
    use mycqu_cli::store::{StateStore, StoreError};
    use rsmycqu::session::Session;
    use std::path::PathBuf;
    use std::time::Duration;

    fn temp_store(tag: &str) -> (StateStore, PathBuf) {
        let base =
            std::env::temp_dir().join(format!("mycqu-cli-test-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        (StateStore::with_base_dir(base.clone()), base)
    }

    #[test]
    fn session_roundtrip_through_protected_storage() {
        let (store, base) = temp_store("roundtrip");
        store.save_session(&Session::new()).expect("save session");
        assert!(base.join("session.bin").exists());
        let loaded = store.load_session().expect("load session");
        assert!(!loaded.is_login());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn challenge_state_persists_and_clears() {
        let (store, base) = temp_store("challenge");
        let stored =
            StoredChallenge::new("cqu-0123456789abcdef0123456789abcdef".to_string(), 1_000);
        store.save_challenge(&stored).expect("save challenge");
        assert_eq!(
            store.load_challenge().expect("load challenge"),
            Some(stored)
        );

        store.clear_challenge().expect("clear challenge");
        assert_eq!(store.load_challenge().expect("load after clear"), None);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn state_lock_excludes_concurrent_holders() {
        let (store, base) = temp_store("lock");
        let (tx, rx) = std::sync::mpsc::channel();
        let holder_store = store.clone();
        let holder = std::thread::spawn(move || {
            let lock = holder_store.acquire_lock().expect("holder acquires lock");
            tx.send(()).unwrap();
            std::thread::sleep(Duration::from_millis(300));
            drop(lock);
        });
        rx.recv().expect("holder acquired lock");

        let err = store
            .acquire_lock_for(Duration::from_millis(50))
            .expect_err("concurrent acquisition must fail");
        assert!(
            matches!(err, StoreError::Locked(_)),
            "expected lock contention, got {err:?}"
        );

        holder.join().expect("holder thread");
        let lock = store.acquire_lock().expect("lock acquirable after release");
        drop(lock);
        let _ = std::fs::remove_dir_all(&base);
    }
    #[test]
    fn clear_session_removes_all_state_files() {
        let (store, base) = temp_store("clear");
        store.save_session(&Session::new()).expect("save session");
        store
            .save_challenge(&StoredChallenge::new("cqu-abc".to_string(), 1))
            .expect("save challenge");
        store
            .save_captcha(b"\x89PNG\r\n\x1a\nfake")
            .expect("save captcha");

        store.clear_session().expect("clear session");
        assert!(!base.join("session.bin").exists());
        assert!(!base.join("challenge.json").exists());
        assert!(!base.join("captcha.png").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn challenge_check_rejects_wrong_or_expired_ids() {
        let stored = StoredChallenge::new("cqu-abc".to_string(), 1_000);
        assert_eq!(
            check_challenge(Some(&stored), "cqu-abc", 1_000),
            ChallengeCheck::Current
        );
        assert_eq!(
            check_challenge(Some(&stored), "cqu-wrong", 1_000),
            ChallengeCheck::Mismatch
        );
        assert_eq!(
            check_challenge(Some(&stored), "cqu-abc", 1_000 + 300),
            ChallengeCheck::Expired
        );
        assert_eq!(
            check_challenge(None, "cqu-abc", 1_000),
            ChallengeCheck::Missing
        );
    }
}
