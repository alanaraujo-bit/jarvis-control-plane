use super::*;

#[test]
fn keep_alive_is_written_the_way_ollama_spells_it() {
    let mut config = RuntimeConfig::default();

    config.keep_alive_minutes = 30;
    assert_eq!(config.keep_alive(), "30m");

    config.keep_alive_minutes = 0;
    assert_eq!(config.keep_alive(), "0", "zero evicts immediately");

    config.keep_alive_minutes = -1;
    assert_eq!(config.keep_alive(), "-1", "negative keeps it resident");
}

/// The model is named on the command line; everything else is configuration.
#[test]
fn a_local_session_names_its_model() {
    let config = RuntimeConfig {
        model: Some("qwen3.8:latest".into()),
        ..RuntimeConfig::default()
    };
    assert_eq!(launch_args(&config), vec!["-m", "qwen3.8:latest"]);
}

/// The bug this configuration was shipped with for exactly one build.
///
/// `wire_api = "chat"` is the obvious value and Codex 0.150.1 refuses it on
/// startup — "no longer supported" — so every local session died before its
/// first prompt. `"responses"` was then measured end to end against Ollama
/// 0.33.2. A test rather than a comment, because the failure is total and
/// silent from the Rust side: the process starts, and the agent never does.
#[test]
fn the_wire_protocol_is_the_one_the_runner_still_accepts() {
    let toml = config_toml(&RuntimeConfig::default(), None);
    assert!(toml.contains(r#"wire_api = "responses""#));
    assert!(!toml.contains(r#"wire_api = "chat""#));
}

/// Our provider is added, never a redefinition of one the runner ships.
#[test]
fn the_configured_provider_does_not_collide_with_a_built_in_one() {
    let toml = config_toml(&RuntimeConfig::default(), None);
    assert!(toml.contains("[model_providers.jarvis-ollama]"));
    assert!(!toml.contains("[model_providers.ollama]"));
}

/// No model chosen is a real state, and the arguments must stay valid in it —
/// the launcher refuses separately, with a message, rather than handing Codex
/// a `-m` with nothing after it.
#[test]
fn no_model_yields_no_dangling_flag() {
    let args = launch_args(&RuntimeConfig::default());
    assert!(args.is_empty());
}

/// The whole reason this runtime writes a configuration file at all.
#[test]
fn a_measured_context_window_is_stated_and_an_unmeasured_one_is_not() {
    let config = RuntimeConfig {
        model: Some("qwen3.8:latest".into()),
        ..RuntimeConfig::default()
    };

    let measured = config_toml(&config, Some(65_536));
    assert!(
        measured.contains("model_context_window = 65536"),
        "without this the runner's metadata is unknown and Codex invents a \
         window four times the real one"
    );

    let unmeasured = config_toml(&config, None);
    assert!(
        !unmeasured.contains("model_context_window"),
        "a guessed context window is exactly what this key exists to prevent"
    );
}

#[test]
fn the_configuration_points_at_the_openai_compatible_path() {
    let config = RuntimeConfig {
        endpoint: "http://127.0.0.1:11434/".into(),
        ..RuntimeConfig::default()
    };
    let toml = config_toml(&config, None);
    assert!(toml.contains("base_url = \"http://127.0.0.1:11434/v1\""));
    assert!(
        !toml.contains("11434//v1"),
        "a trailing slash on the endpoint must not double up"
    );
}

/// Endpoint and model are user-typed text written into a file a program parses.
#[test]
fn user_typed_values_are_escaped_rather_than_interpolated() {
    let config = RuntimeConfig {
        model: Some("weird\"name\\here".into()),
        ..RuntimeConfig::default()
    };
    let toml = config_toml(&config, None);
    assert!(toml.contains(r#"model = "weird\"name\\here""#));
}

#[test]
fn the_sandbox_and_approval_choices_reach_the_configuration() {
    let config = RuntimeConfig {
        sandbox: SandboxMode::ReadOnly,
        approval: ApprovalPolicy::Never,
        ..RuntimeConfig::default()
    };
    let toml = config_toml(&config, None);
    assert!(toml.contains("sandbox_mode = \"read-only\""));
    assert!(toml.contains("approval_policy = \"never\""));
}

/// A local agent exists to change code. Starting every session read-only would
/// make the provider's first act be a failure to do the thing it is for.
#[test]
fn the_default_session_can_write_inside_the_workspace_and_no_further() {
    let config = RuntimeConfig::default();
    assert_eq!(config.sandbox, SandboxMode::WorkspaceWrite);
    assert_ne!(
        config.sandbox,
        SandboxMode::DangerFullAccess,
        "full machine access is a decision, never an inheritance"
    );
}

/// The configuration root is this app's, not the user's.
#[test]
fn the_runtime_never_writes_into_the_users_own_codex_home() {
    let temp = tempfile::tempdir().unwrap();
    let home = prepare(temp.path(), &RuntimeConfig::default(), Some(65_536)).unwrap();

    assert!(home.starts_with(temp.path()));
    assert!(home.join("config.toml").is_file());
    assert!(
        transcript_root(temp.path()).starts_with(temp.path()),
        "local rollouts land in our own tree, so correlation can never match a \
         cloud Codex session's rollout by cwd and start time"
    );
}

/// Saving twice must leave one current file, not append to a growing one.
#[test]
fn preparing_again_replaces_the_configuration() {
    let temp = tempfile::tempdir().unwrap();
    prepare(temp.path(), &RuntimeConfig::default(), Some(65_536)).unwrap();
    let config = RuntimeConfig {
        sandbox: SandboxMode::ReadOnly,
        ..RuntimeConfig::default()
    };
    let home = prepare(temp.path(), &config, None).unwrap();

    let written = std::fs::read_to_string(home.join("config.toml")).unwrap();
    assert!(written.contains("read-only"));
    assert!(!written.contains("workspace-write"));
    assert!(!written.contains("model_context_window"));
}
