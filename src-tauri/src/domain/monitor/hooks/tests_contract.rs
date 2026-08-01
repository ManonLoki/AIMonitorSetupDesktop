use std::time::Duration;

use super::{
    ai_tool_descriptors, hook_changed_write_outcome, hook_config_write_result,
    hook_requires_review, hook_restart_required, protocol, release_settle_delay,
    session_start_revives_tombstone,
};
use crate::domain::monitor::{
    AiTool, DEFAULT_DISCOVERY_INTERVAL_MINUTES, DEFAULT_PROFILE_SLOT, HookBehavior,
    HookWriteOutcome, MAX_DISCOVERY_INTERVAL_MINUTES, MAX_PROFILE_SLOT,
    MIN_DISCOVERY_INTERVAL_MINUTES, MIN_PROFILE_SLOT, monitor_capabilities,
};

const TOOL_CONTRACTS: [(AiTool, &str, HookWriteOutcome); 13] = [
    (
        AiTool::Codex,
        "Codex",
        HookWriteOutcome::CodexReviewRequired,
    ),
    (AiTool::ClaudeCode, "Claude Code", HookWriteOutcome::Active),
    (AiTool::Cursor, "Cursor", HookWriteOutcome::Active),
    (AiTool::OpenCode, "OpenCode", HookWriteOutcome::Active),
    (
        AiTool::WorkBuddy,
        "WorkBuddy",
        HookWriteOutcome::WorkBuddyReviewRequired,
    ),
    (
        AiTool::Hermes,
        "Hermes",
        HookWriteOutcome::HermesEnableRequired,
    ),
    (
        AiTool::OpenClaw,
        "OpenClaw",
        HookWriteOutcome::OpenClawEnableRequired,
    ),
    (
        AiTool::CodeBuddy,
        "CodeBuddy",
        HookWriteOutcome::CodeBuddyReviewRequired,
    ),
    (
        AiTool::QwenCode,
        "Qwen Code",
        HookWriteOutcome::RestartRequired,
    ),
    (
        AiTool::KimiCode,
        "Kimi Code",
        HookWriteOutcome::RestartRequired,
    ),
    (AiTool::Qoder, "Qoder", HookWriteOutcome::Active),
    (
        AiTool::GeminiCli,
        "Gemini CLI",
        HookWriteOutcome::RestartRequired,
    ),
    (
        AiTool::GitHubCopilot,
        "GitHub Copilot CLI",
        HookWriteOutcome::RestartRequired,
    ),
];

#[test]
fn tool_catalog_and_write_outcomes_are_protocol_owned_and_complete() {
    let descriptors = ai_tool_descriptors();
    assert_eq!(descriptors.len(), AiTool::ALL.len());

    for ((expected_tool, expected_name, expected_outcome), descriptor) in
        TOOL_CONTRACTS.into_iter().zip(descriptors)
    {
        assert_eq!(descriptor.tool, expected_tool);
        assert_eq!(descriptor.name, expected_name);
        assert_eq!(protocol(expected_tool).tool(), expected_tool);
        assert_eq!(hook_changed_write_outcome(expected_tool), expected_outcome);
        assert_eq!(
            hook_requires_review(expected_tool),
            expected_outcome.requires_review()
        );
        assert_eq!(
            hook_restart_required(expected_tool),
            expected_outcome.restart_required()
        );

        let changed = hook_config_write_result(expected_tool, "config".to_owned(), true);
        assert_eq!(changed.outcome, expected_outcome);
        assert!(changed.config_changed);
        let unchanged = hook_config_write_result(expected_tool, "config".to_owned(), false);
        assert_eq!(unchanged.outcome, HookWriteOutcome::Unchanged);
        assert!(!unchanged.config_changed);
        assert!(!unchanged.requires_review);
        assert!(!unchanged.restart_required);
    }
}

#[test]
fn monitor_capabilities_serialize_ranges_and_behavior_order_from_domain_constants() {
    let capabilities = monitor_capabilities();

    assert_eq!(
        capabilities
            .ai_tools
            .iter()
            .map(|descriptor| descriptor.tool)
            .collect::<Vec<_>>(),
        AiTool::ALL
    );
    assert_eq!(
        capabilities.hook_behaviors,
        vec![
            HookBehavior::Idle,
            HookBehavior::Running,
            HookBehavior::Asking,
            HookBehavior::Error,
        ]
    );
    assert_eq!(
        (
            capabilities.discovery_interval.default,
            capabilities.discovery_interval.min,
            capabilities.discovery_interval.max,
        ),
        (
            DEFAULT_DISCOVERY_INTERVAL_MINUTES,
            MIN_DISCOVERY_INTERVAL_MINUTES,
            MAX_DISCOVERY_INTERVAL_MINUTES,
        )
    );
    assert_eq!(
        (
            capabilities.profile_slot.default,
            capabilities.profile_slot.min,
            capabilities.profile_slot.max,
        ),
        (DEFAULT_PROFILE_SLOT, MIN_PROFILE_SLOT, MAX_PROFILE_SLOT)
    );

    let serialized = serde_json::to_value(capabilities).unwrap();
    assert_eq!(serialized["aiTools"][0]["name"], "Codex");
    assert_eq!(serialized["discoveryInterval"]["default"], 1);
    assert_eq!(serialized["profileSlot"]["max"], 25);
    assert_eq!(
        serialized["imageUploadAccept"]["mimeTypes"],
        serde_json::json!([
            "image/bmp",
            "image/x-ms-bmp",
            "image/jpeg",
            "image/gif",
            "image/png",
            "image/webp",
        ])
    );
    assert_eq!(
        serialized["imageUploadAccept"]["extensions"],
        serde_json::json!([".bmp", ".jpg", ".jpeg", ".gif", ".png", ".webp"])
    );
    assert_eq!(
        serialized["hookBehaviors"],
        serde_json::json!(["idle", "running", "asking", "error"])
    );
}

#[test]
fn cursor_alone_declares_a_release_handoff_grace() {
    assert_eq!(
        release_settle_delay(AiTool::Cursor),
        Duration::from_millis(250)
    );
    for tool in AiTool::ALL {
        if tool != AiTool::Cursor {
            assert_eq!(release_settle_delay(tool), Duration::ZERO, "{tool:?}");
        }
    }
}

#[test]
fn cursor_alone_rejects_a_session_start_over_an_end_tombstone() {
    assert!(!session_start_revives_tombstone(AiTool::Cursor));
    for tool in AiTool::ALL {
        if tool != AiTool::Cursor {
            assert!(session_start_revives_tombstone(tool), "{tool:?}");
        }
    }
}
