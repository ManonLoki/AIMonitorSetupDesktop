use super::{HookEvent, HookEventKind, HookProtocol, managed_hook_marker};
use crate::domain::monitor::{AiTool, DEFAULT_HOOK_RELAY_PORT, HookBehavior, HookConfigPreview};

pub(super) static HERMES: HermesProtocol = HermesProtocol;

pub(super) struct HermesProtocol;

// Hermes 的 observer hooks 是官方插件接口。事件名称保持与 Hermes
// hermes_cli.plugins.VALID_HOOKS 一致，插件只做只读状态观察，不改变 Agent 行为。
const EVENTS: &[HookEvent] = &[
    HookEvent::new("on_session_start", HookEventKind::SessionStart),
    HookEvent::new("pre_llm_call", HookEventKind::WorkStart),
    HookEvent::new(
        "pre_tool_call",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        "post_tool_call",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new(
        "pre_approval_request",
        HookEventKind::State(HookBehavior::Asking),
    ),
    HookEvent::new(
        "post_approval_response",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        "api_request_error",
        HookEventKind::State(HookBehavior::Error),
    ),
    HookEvent::new("post_llm_call", HookEventKind::Stop),
    HookEvent::new("on_session_end", HookEventKind::Stop),
    HookEvent::new("on_session_finalize", HookEventKind::SessionEnd),
    HookEvent::new("on_session_reset", HookEventKind::SessionEnd),
];

impl HookProtocol for HermesProtocol {
    fn tool(&self) -> AiTool {
        AiTool::Hermes
    }

    fn name(&self) -> &'static str {
        "Hermes"
    }

    fn slug(&self) -> &'static str {
        "hermes"
    }

    fn config_filename(&self) -> &'static str {
        "plugins/aimonitor/__init__.py"
    }

    fn preview_filename(&self) -> &'static str {
        ".hermes/plugins/aimonitor/__init__.py"
    }

    fn events(&self) -> &'static [HookEvent] {
        EVENTS
    }

    // 用户插件默认不加载；写入后必须通过 Hermes CLI 明确信任并启用。
    fn requires_review(&self) -> bool {
        true
    }

    // 已运行的 Hermes 进程不会重新扫描新插件，需要新会话或重启进程。
    fn restart_required(&self) -> bool {
        true
    }

    fn standalone_config(&self) -> Option<String> {
        let marker = managed_hook_marker(AiTool::Hermes);
        Some(format!(
            r#"# {marker}
\"\"\"Relay Hermes observer lifecycle events to the local AIMonitor app.\"\"\"

from __future__ import annotations

import json
import urllib.request

ENDPOINT = \"http://127.0.0.1:{DEFAULT_HOOK_RELAY_PORT}/api/hooks/hermes\"


def _send(hook_event: str, **kwargs) -> None:
    session_id = kwargs.get(\"session_id\") or kwargs.get(\"session_key\")
    turn_id = kwargs.get(\"turn_id\") or kwargs.get(\"task_id\")
    if hook_event == \"on_session_reset\":
        session_id = kwargs.get(\"old_session_id\") or session_id
    status = kwargs.get(\"status\") or kwargs.get(\"choice\") or kwargs.get(\"reason\")
    payload = json.dumps({{
        \"hook_event_name\": hook_event,
        \"session_id\": session_id,
        \"turn_id\": turn_id,
        \"status\": str(status) if status is not None else None,
    }}).encode(\"utf-8\")
    request = urllib.request.Request(
        ENDPOINT,
        data=payload,
        headers={{
            \"Content-Type\": \"application/json\",
            \"X-AIMonitor-Hook-Type\": hook_event,
        }},
        method=\"POST\",
    )
    try:
        with urllib.request.urlopen(request, timeout=1):
            pass
    except Exception:
        # AIMonitor 未运行时保持 fail-open，不影响 Hermes Agent。
        pass


def register(ctx) -> None:
    for hook_event in (
        \"on_session_start\",
        \"pre_llm_call\",
        \"pre_tool_call\",
        \"post_tool_call\",
        \"pre_approval_request\",
        \"post_approval_response\",
        \"api_request_error\",
        \"post_llm_call\",
        \"on_session_end\",
        \"on_session_finalize\",
        \"on_session_reset\",
    ):
        ctx.register_hook(
            hook_event,
            lambda _event=hook_event, **kwargs: _send(_event, **kwargs),
        )
"#
        ))
    }

    fn auxiliary_configs(&self) -> Vec<HookConfigPreview> {
        let marker = managed_hook_marker(AiTool::Hermes);
        vec![HookConfigPreview {
            filename: "plugins/aimonitor/plugin.yaml".to_owned(),
            content: format!(
                r#"name: aimonitor
version: 1.0.0
description: \"{marker} - relay Hermes lifecycle state to AIMonitor\"
author: AIMonitor
hooks:
  - on_session_start
  - pre_llm_call
  - pre_tool_call
  - post_tool_call
  - pre_approval_request
  - post_approval_response
  - api_request_error
  - post_llm_call
  - on_session_end
  - on_session_finalize
  - on_session_reset
"#
            ),
        }]
    }
}
