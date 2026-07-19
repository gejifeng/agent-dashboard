use serde::Serialize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UiLanguage {
    #[default]
    ZhCn,
    En,
}

impl UiLanguage {
    pub fn parse(value: Option<&str>) -> Self {
        match value
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "en" | "en-us" | "en_us" | "english" => Self::En,
            _ => Self::ZhCn,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ZhCn => "zh-CN",
            Self::En => "en",
        }
    }

    pub fn output_name(self) -> &'static str {
        match self {
            Self::ZhCn => "Simplified Chinese",
            Self::En => "English",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NameSource {
    Manual,
    Agent,
    #[default]
    Fallback,
}

impl NameSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Agent => "agent",
            Self::Fallback => "fallback",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    OpenCode,
    ClaudeCode,
    Codex,
    #[default]
    Generic,
}

impl AgentKind {
    pub fn from_command(command: &str) -> Self {
        let command = command.to_ascii_lowercase();
        if command.contains("opencode") {
            Self::OpenCode
        } else if command.contains("claude") {
            Self::ClaudeCode
        } else if command.contains("codex") {
            Self::Codex
        } else {
            Self::Generic
        }
    }

    pub fn parse(value: Option<&str>) -> Self {
        match value
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "opencode" | "open_code" => Self::OpenCode,
            "claude" | "claude-code" | "claude_code" => Self::ClaudeCode,
            "codex" => Self::Codex,
            _ => Self::Generic,
        }
    }

    pub fn label(self, language: UiLanguage) -> &'static str {
        match self {
            Self::OpenCode => "OpenCode",
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
            Self::Generic if language == UiLanguage::En => "Terminal",
            Self::Generic => "终端",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenCode => "opencode",
            Self::ClaudeCode => "claude_code",
            Self::Codex => "codex",
            Self::Generic => "generic",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentState {
    Working,
    Idle,
    WaitingApproval,
    Retrying,
    Error,
    #[default]
    Unknown,
}

impl AgentState {
    pub fn parse(value: Option<&str>) -> Self {
        match value
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "working" | "busy" | "running" | "tool_start" | "prompt_submit" => Self::Working,
            "idle" | "stop" | "completed" | "complete" | "session_idle" => Self::Idle,
            "waiting_approval" | "approval" | "permission" | "permission_request" | "waiting" => {
                Self::WaitingApproval
            }
            "retry" | "retrying" => Self::Retrying,
            "error" | "failed" | "failure" => Self::Error,
            _ => Self::Unknown,
        }
    }

    pub fn from_event(event: &str) -> Self {
        let normalized = event
            .trim()
            .replace(['.', '-', ' '], "_")
            .to_ascii_lowercase();
        if normalized.contains("permission") || normalized.contains("approval") {
            Self::WaitingApproval
        } else if normalized.contains("error")
            || normalized.contains("failure")
            || normalized.contains("failed")
        {
            Self::Error
        } else if normalized.contains("retry") {
            Self::Retrying
        } else if normalized == "stop"
            || normalized.contains("session_idle")
            || normalized.contains("status_idle")
            || normalized.contains("turn_complete")
            || normalized.contains("task_completed")
            || normalized.contains("session_end")
            || normalized.contains("session_start")
            || normalized.contains("session_created")
        {
            Self::Idle
        } else if normalized.contains("prompt")
            || normalized.contains("tool")
            || normalized.contains("session_status")
            || normalized.contains("status_busy")
            || normalized.contains("busy")
            || normalized.contains("working")
        {
            Self::Working
        } else {
            Self::Unknown
        }
    }

    pub fn status(self) -> Option<&'static str> {
        match self {
            Self::Working => Some("ok"),
            Self::Idle => Some("idle"),
            Self::WaitingApproval | Self::Error => Some("err"),
            Self::Retrying => Some("warn"),
            Self::Unknown => None,
        }
    }

    pub fn default_summary(self, agent: AgentKind, language: UiLanguage) -> String {
        let agent = agent.label(language);
        match (self, language) {
            (Self::Working, UiLanguage::En) => format!("{agent} is working"),
            (Self::Idle, UiLanguage::En) => format!("{agent} is idle and waiting for input"),
            (Self::WaitingApproval, UiLanguage::En) => {
                format!("{agent} is waiting for user approval")
            }
            (Self::Retrying, UiLanguage::En) => format!("{agent} is retrying"),
            (Self::Error, UiLanguage::En) => format!("{agent} encountered an error"),
            (Self::Unknown, UiLanguage::En) => format!("{agent} status was updated"),
            (Self::Working, UiLanguage::ZhCn) => format!("{agent} 正在执行任务"),
            (Self::Idle, UiLanguage::ZhCn) => format!("{agent} 已停止执行，等待输入"),
            (Self::WaitingApproval, UiLanguage::ZhCn) => format!("{agent} 等待用户确认"),
            (Self::Retrying, UiLanguage::ZhCn) => format!("{agent} 正在重试"),
            (Self::Error, UiLanguage::ZhCn) => format!("{agent} 执行出错，需要检查"),
            (Self::Unknown, UiLanguage::ZhCn) => format!("{agent} 状态已更新"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::Idle => "idle",
            Self::WaitingApproval => "waiting_approval",
            Self::Retrying => "retrying",
            Self::Error => "error",
            Self::Unknown => "unknown",
        }
    }
}

pub fn llm_input(agent: AgentKind, state: AgentState, language: UiLanguage, text: &str) -> String {
    format!(
        "Agent type: {}\nAdapter state: {} (this state code is authoritative; only summarize the concrete task)\nRequired summary language: {}\nSemantic terminal screen:\n{}",
        agent.label(language),
        state.as_str(),
        language.output_name(),
        text
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_supported_agents_from_commands() {
        assert_eq!(
            AgentKind::from_command("opencode --auto"),
            AgentKind::OpenCode
        );
        assert_eq!(AgentKind::from_command("claude"), AgentKind::ClaudeCode);
        assert_eq!(
            AgentKind::from_command("codex --no-alt-screen"),
            AgentKind::Codex
        );
        assert_eq!(AgentKind::from_command("pwsh"), AgentKind::Generic);
    }

    #[test]
    fn maps_structured_events_to_states() {
        assert_eq!(
            AgentState::from_event("permission.asked"),
            AgentState::WaitingApproval
        );
        assert_eq!(AgentState::from_event("session.idle"), AgentState::Idle);
        assert_eq!(AgentState::from_event("PreToolUse"), AgentState::Working);
        assert_eq!(
            AgentState::from_event("PostToolUseFailure"),
            AgentState::Error
        );
    }

    #[test]
    fn localizes_default_agent_summaries() {
        assert_eq!(
            AgentState::Idle.default_summary(AgentKind::Codex, UiLanguage::En),
            "Codex is idle and waiting for input"
        );
        assert_eq!(
            AgentState::Retrying.default_summary(AgentKind::OpenCode, UiLanguage::ZhCn),
            "OpenCode 正在重试"
        );
    }
}
