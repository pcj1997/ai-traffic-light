import { invoke } from "@tauri-apps/api/core";
import "./styles.css";

type Status =
  | "idle"
  | "working"
  | "waiting"
  | "completed"
  | "error";

interface StatusSnapshot {
  state: Status;
  label: string;
  message: string;
  session_count: number;
  updated_at: number;
  sessions: SessionSnapshot[];
}

interface SessionSnapshot {
  id: string;
  client: string;
  title: string;
  state: Status;
  label: string;
  message: string;
  updated_at: number;
}

const lights = Array.from(document.querySelectorAll<HTMLElement>(".light"));
const statusLabel = document.querySelector<HTMLElement>("#status-label");
const sessionCount = document.querySelector<HTMLElement>("#session-count");
const sessionList = document.querySelector<HTMLUListElement>("#session-list");
const hooksWarning = document.querySelector<HTMLElement>("#hooks-warning");
const hooksWarningMessage = document.querySelector<HTMLElement>(
  "#hooks-warning-message",
);
const hooksStatus = document.querySelector<HTMLElement>("#hooks-status");
const setupButton = document.querySelector<HTMLButtonElement>("#setup-hooks");
const clearSessionsButton =
  document.querySelector<HTMLButtonElement>("#clear-sessions");
let hooksSuccessVisibleUntil = 0;

function clientLabel(client: string) {
  if (client === "codex") return "Codex";
  if (client === "claude") return "Claude";
  return "CodeBuddy";
}

function render(snapshot: StatusSnapshot) {
  document.body.dataset.state = snapshot.state;

  for (const light of lights) {
    light.classList.toggle("active", light.dataset.state === snapshot.state);
  }

  if (statusLabel) statusLabel.textContent = snapshot.label;
  if (sessionCount) {
    sessionCount.textContent =
      snapshot.session_count > 0
        ? `${snapshot.session_count} 个会话状态`
        : "暂无会话记录";
  }
  if (sessionList) {
    sessionList.replaceChildren(
      ...snapshot.sessions.map((session) => {
        const item = document.createElement("li");
        item.className = `session-item session-${session.state}`;

        const dot = document.createElement("span");
        dot.className = "session-dot";

        const content = document.createElement("span");
        content.className = "session-content";

        const remove = document.createElement("button");
        remove.className = "session-remove";
        remove.type = "button";
        remove.title = "删除会话记录";
        remove.setAttribute("aria-label", `删除 ${session.title}`);
        remove.textContent = "×";
        remove.addEventListener("click", async () => {
          remove.disabled = true;
          await invoke("remove_session", { id: session.id });
          await refresh();
        });

        const heading = document.createElement("span");
        heading.className = "session-heading";

        const client = document.createElement("span");
        client.className = "session-client";
        client.textContent = clientLabel(session.client);

        const title = document.createElement("strong");
        title.textContent = session.title;

        const details = document.createElement("span");
        details.className = "session-details";

        const message = document.createElement("span");
        message.textContent = session.message || session.label;

        heading.append(title);
        details.append(client, message);
        content.append(heading, details);
        item.append(dot, content, remove);
        return item;
      }),
    );
  }
  if (clearSessionsButton) {
    clearSessionsButton.hidden = snapshot.session_count === 0;
  }
}

async function refresh() {
  try {
    render(await invoke<StatusSnapshot>("get_status"));
  } catch (error) {
    console.error("Failed to refresh AI client status", error);
  }
}

async function refreshHooksStatus() {
  if (!hooksWarning) return;
  try {
    const installed = await invoke<boolean>("hooks_installed");
    const showSuccess = installed && Date.now() < hooksSuccessVisibleUntil;
    hooksWarning.hidden = installed && !showSuccess;
    if (hooksWarningMessage) hooksWarningMessage.hidden = installed;
    if (setupButton) setupButton.hidden = installed;
  } catch (error) {
    hooksWarning.hidden = false;
    if (hooksWarningMessage) hooksWarningMessage.hidden = false;
    if (setupButton) setupButton.hidden = false;
    console.error("Failed to refresh Hooks status", error);
  }
}

function setHooksStatus(message: string, state: "busy" | "error" | "success") {
  if (!hooksStatus) return;
  hooksStatus.hidden = false;
  hooksStatus.dataset.state = state;
  hooksStatus.textContent = message;
}

setupButton?.addEventListener("click", async () => {
  setupButton.disabled = true;
  setupButton.textContent = "正在安装…";
  setHooksStatus("正在写入 Hooks 配置，请稍候。", "busy");
  try {
    await invoke<string>("install_hooks");
    hooksSuccessVisibleUntil = Date.now() + 15000;
    setHooksStatus(
      "Hooks 已安装。请重启 AI 客户端并新建会话。Codex 还需在新会话输入 /hooks，检查并信任 AI Traffic Light Hooks。",
      "success",
    );
    await refreshHooksStatus();
  } catch (error) {
    setHooksStatus(`安装失败：${String(error)}`, "error");
  } finally {
    setupButton.textContent = "安装 Hooks";
    setupButton.disabled = false;
  }
});

clearSessionsButton?.addEventListener("click", async () => {
  clearSessionsButton.disabled = true;
  try {
    await invoke("clear_session_history");
    await refresh();
  } finally {
    clearSessionsButton.disabled = false;
  }
});

refresh();
refreshHooksStatus();
window.setInterval(refresh, 500);
window.setInterval(refreshHooksStatus, 5000);
