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
const setupButton = document.querySelector<HTMLButtonElement>("#setup-hooks");
const clearSessionsButton =
  document.querySelector<HTMLButtonElement>("#clear-sessions");

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

        const title = document.createElement("strong");
        title.textContent = session.title;

        const message = document.createElement("span");
        message.textContent = session.message || session.label;

        content.append(title, message);
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
    console.error("Failed to refresh CodeBuddy status", error);
  }
}

async function refreshHooksStatus() {
  if (!hooksWarning) return;
  try {
    hooksWarning.hidden = await invoke<boolean>("hooks_installed");
  } catch (error) {
    hooksWarning.hidden = false;
    console.error("Failed to refresh Hooks status", error);
  }
}

setupButton?.addEventListener("click", async () => {
  setupButton.disabled = true;
  try {
    setupButton.textContent = await invoke<string>("install_hooks");
    await refreshHooksStatus();
  } catch (error) {
    setupButton.textContent = `安装失败: ${String(error)}`;
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
