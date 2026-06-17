import { invoke } from "@tauri-apps/api/core";
import {
  currentMonitor,
  getCurrentWindow,
  PhysicalPosition,
} from "@tauri-apps/api/window";
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
  created_at: number;
}

interface RemoteBridgeStatus {
  enabled: boolean;
  port: number;
  local_url: string;
  remote_url: string;
  ssh_reverse_tunnel_command: string;
  remote_installer_path: string;
  remote_install_command: string;
  error_message: string;
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
const remoteBridgeSummary = document.querySelector<HTMLElement>(
  "#remote-bridge-summary",
);
const remoteBridgeCommand = document.querySelector<HTMLElement>(
  "#remote-bridge-command",
);
const remoteBridgeStatus = document.querySelector<HTMLElement>(
  "#remote-bridge-status",
);
const prepareRemoteBridgeButton = document.querySelector<HTMLButtonElement>(
  "#prepare-remote-bridge",
);
const appWindow = getCurrentWindow();
const panelShift = 158;
let hooksSuccessVisibleUntil = 0;
let panelPlacementChanging = false;

function clientLabel(client: string) {
  if (client === "codex") return "Codex";
  if (client === "claude") return "Claude";
  return "CodeBuddy";
}

async function updatePanelPlacement(position?: PhysicalPosition) {
  if (panelPlacementChanging) return;

  const [windowPosition, monitor, scaleFactor] = await Promise.all([
    position ? Promise.resolve(position) : appWindow.outerPosition(),
    currentMonitor(),
    appWindow.scaleFactor(),
  ]);
  if (!monitor) return;

  const panelOnLeft = document.body.dataset.panelSide === "left";
  const housingCenter =
    windowPosition.x + (40 + (panelOnLeft ? panelShift : 0)) * scaleFactor;
  const monitorCenter =
    monitor.workArea.position.x + monitor.workArea.size.width / 2;
  const shouldPlacePanelOnLeft = housingCenter > monitorCenter;
  if (panelOnLeft === shouldPlacePanelOnLeft) return;

  document.body.dataset.panelSide = shouldPlacePanelOnLeft ? "left" : "right";
  panelPlacementChanging = true;
  try {
    const offset = (shouldPlacePanelOnLeft ? -panelShift : panelShift) * scaleFactor;
    await appWindow.setPosition(
      new PhysicalPosition(windowPosition.x + offset, windowPosition.y),
    );
  } finally {
    panelPlacementChanging = false;
  }
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
    const clients = Array.from(
      new Set(snapshot.sessions.map((session) => session.client)),
    ).sort((left, right) => {
      const firstOpenedAt = (client: string) =>
        Math.min(
          ...snapshot.sessions
            .filter((session) => session.client === client)
            .map((session) => session.created_at),
        );
      return firstOpenedAt(left) - firstOpenedAt(right);
    });
    sessionList.replaceChildren(
      ...clients.flatMap((client) => {
        const groupTitle = document.createElement("li");
        groupTitle.className = "session-group-title";
        groupTitle.textContent = clientLabel(client);

        const sessions = snapshot.sessions
          .filter((session) => session.client === client)
          .map((session) => {
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

            const title = document.createElement("strong");
            title.textContent = session.title;

            const message = document.createElement("span");
            message.textContent = session.message || session.label;

            heading.append(title);
            content.append(heading, message);
            item.append(dot, content, remove);
            return item;
          });

        return [groupTitle, ...sessions];
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

function setRemoteBridgeStatus(
  message: string,
  state: "busy" | "error" | "success",
) {
  if (!remoteBridgeStatus) return;
  remoteBridgeStatus.hidden = false;
  remoteBridgeStatus.dataset.state = state;
  remoteBridgeStatus.textContent = message;
}

async function refreshRemoteBridgeStatus() {
  try {
    const status = await invoke<RemoteBridgeStatus>("get_remote_bridge_status");
    if (remoteBridgeSummary) {
      remoteBridgeSummary.textContent = status.enabled
        ? `本机端口 ${status.port}`
        : `桥接未启动${status.error_message ? `：${status.error_message}` : ""}`;
    }
    if (remoteBridgeCommand) {
      remoteBridgeCommand.textContent = status.ssh_reverse_tunnel_command;
      remoteBridgeCommand.hidden = !status.enabled;
    }
    if (prepareRemoteBridgeButton) {
      prepareRemoteBridgeButton.disabled = !status.enabled;
    }
  } catch (error) {
    if (remoteBridgeSummary) remoteBridgeSummary.textContent = "桥接状态读取失败";
    console.error("Failed to refresh remote bridge status", error);
  }
}

setupButton?.addEventListener("click", async () => {
  setupButton.disabled = true;
  setupButton.textContent = "正在安装…";
  setHooksStatus("正在写入 Hooks 配置，请稍候。", "busy");
  try {
    await invoke<string>("install_hooks");
    hooksSuccessVisibleUntil = Date.now() + 15000;
    setHooksStatus(
      "Hooks 已安装。请重启 AI 客户端并新建会话。Codex 桌面版请按新对话顶部提示信任 Hooks；CLI 可输入 /hooks 检查并信任。",
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

prepareRemoteBridgeButton?.addEventListener("click", async () => {
  prepareRemoteBridgeButton.disabled = true;
  setRemoteBridgeStatus("正在生成远端安装脚本…", "busy");
  try {
    const status = await invoke<RemoteBridgeStatus>(
      "prepare_remote_codebuddy_bridge",
    );
    setRemoteBridgeStatus(
      `已生成：${status.remote_installer_path}。先保持 SSH 反向隧道，再把脚本复制到服务器运行。`,
      "success",
    );
    await refreshRemoteBridgeStatus();
  } catch (error) {
    setRemoteBridgeStatus(`生成失败：${String(error)}`, "error");
  } finally {
    prepareRemoteBridgeButton.disabled = false;
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
refreshRemoteBridgeStatus();
updatePanelPlacement().catch((error) => {
  console.error("Failed to update panel placement", error);
});
appWindow.onMoved(({ payload }) => {
  updatePanelPlacement(payload).catch((error) => {
    console.error("Failed to update panel placement", error);
  });
});
window.setInterval(refresh, 500);
window.setInterval(refreshHooksStatus, 5000);
window.setInterval(refreshRemoteBridgeStatus, 5000);
