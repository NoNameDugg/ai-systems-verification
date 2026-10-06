//! External review (2026-10) reproduction — finding #17
//!
//! The Dockerfile documents environment variables that the code never reads.
//! This test pins the contract: every `OANDA_*` / `ASTRA_*` variable the
//! Dockerfile mentions (as `-e NAME=` in the run example or `ENV NAME=`) must
//! appear as a string literal in the code that reads environment variables.
//!
//! Committed `#[ignore]`d-and-failing on purpose; run with `-- --ignored`.

use std::collections::BTreeSet;

const DOCKERFILE: &str = include_str!("../../Dockerfile");
const CONFIG_RS: &str = include_str!("../../src/core/config.rs");
const MAIN_RS: &str = include_str!("../../src/main.rs");

/// Variables the Dockerfile sets for Docker or the Rust runtime itself, not
/// for this application's code.
const NOT_APP_VARS: &[&str] = &["APP_HOME", "RUST_BACKTRACE"];

fn dockerfile_env_names() -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for raw in DOCKERFILE.lines() {
        let line = raw.trim().trim_start_matches('#').trim();
        // `-e NAME=value` (docker run example in the header comment)
        if let Some(rest) = line.strip_prefix("-e ") {
            if let Some(name) = rest.split('=').next() {
                names.insert(name.trim().to_string());
            }
        }
        // `ENV NAME=value`
        if let Some(rest) = line.strip_prefix("ENV ") {
            if let Some(name) = rest.split('=').next() {
                names.insert(name.trim().to_string());
            }
        }
    }
    names
        .into_iter()
        .filter(|n| n.starts_with("OANDA_") || n.starts_with("ASTRA_"))
        .filter(|n| !NOT_APP_VARS.contains(&n.as_str()))
        .collect()
}

fn code_reads(name: &str) -> bool {
    let needle = format!("\"{}\"", name);
    CONFIG_RS.contains(&needle) || MAIN_RS.contains(&needle)
}

#[test]
fn dockerfile_mentions_app_env_vars() {
    // sanity: the parser finds the variables we know are there
    let names = dockerfile_env_names();
    assert!(names.contains("ASTRA_FLASH_CONFIG"), "{:?}", names);
    assert!(names.contains("ASTRA_FLASH_REDIS_URL"), "{:?}", names);
}

#[test]
#[ignore = "review #17: OANDA_API_TOKEN, OANDA_ACCOUNT_ID, ASTRA_ENV, ASTRA_LOG_LEVEL are documented but never read"]
fn every_documented_env_var_is_read_by_the_code() {
    let unread: Vec<String> = dockerfile_env_names()
        .into_iter()
        .filter(|n| !code_reads(n))
        .collect();
    assert!(
        unread.is_empty(),
        "Dockerfile documents env vars the code never reads: {:?}",
        unread
    );
}

/// Adjacent to #17: the documented `docker run` leaves `api_key: ""` in the
/// YAML, which deserialised to `Some("")` and slipped past the
/// "not configured" guard, so the binary sent `Authorization: Bearer ` and
/// looped on HTTP 401.
#[test]
#[ignore = "review #17 (adjacent): an empty credential string must read as not configured"]
fn empty_credential_strings_are_not_configured() {
    use astra_flash::core::config::FlashConfig;

    let yaml = "exchanges:\n  oanda:\n    enabled: true\n    api_key: \"\"\n    account_id: \"\"\n";
    let cfg = FlashConfig::from_yaml(yaml).expect("yaml parses");

    assert_eq!(cfg.exchanges.oanda.api_key, None);
    assert_eq!(cfg.exchanges.oanda.account_id, None);
}
