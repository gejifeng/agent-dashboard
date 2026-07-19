// Copy to .opencode/plugins/agent-dashboard.js in a project or to the global plugin directory.
// OpenCode lifecycle events provide structured state without parsing the terminal screen.
export const AgentDashboardPlugin = async () => ({
  event: async ({ event }) => {
    const sessionID = process.env.AGENT_DASHBOARD_SESSION_ID
    const url = process.env.AGENT_DASHBOARD_EVENT_URL
    if (!sessionID || !url) return

    const supported = [
      "session.created", "session.updated", "session.status", "session.idle", "session.error",
      "permission.asked", "tool.execute.before", "tool.execute.after",
    ]
    if (!supported.includes(event.type)) return

    const properties = event.properties || {}
    const session = properties.info || properties.session || {}
    const statusValue = properties.status?.type || properties.status
    const reportedEvent = event.type === "session.status" && statusValue
      ? `session.status.${statusValue}`
      : event.type
    const summaryZhCn = event.type === "permission.asked"
      ? "OpenCode 等待用户授权"
      : event.type === "tool.execute.before"
        ? `OpenCode 正在调用 ${properties.tool || "工具"}`
        : event.type === "session.error"
          ? `OpenCode 执行出错：${properties.error?.message || properties.error || "请检查终端"}`
          : undefined
    const summaryEn = event.type === "permission.asked"
      ? "OpenCode is waiting for user approval"
      : event.type === "tool.execute.before"
        ? `OpenCode is calling ${properties.tool || "a tool"}`
        : event.type === "session.error"
          ? `OpenCode error: ${properties.error?.message || properties.error || "check the terminal"}`
          : undefined
    try {
      await fetch(url, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          dashboard_session_id: sessionID,
          agent: "opencode",
          event: reportedEvent,
          summary_zh_cn: summaryZhCn,
          summary_en: summaryEn,
          agent_session_id: properties.sessionID || properties.sessionId || session.id,
          session_title: session.title || properties.title,
        }),
      })
    } catch (_) {
      // A stopped dashboard must never affect OpenCode.
    }
  },
})
