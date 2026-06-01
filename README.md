# CodeBuddy Light

CodeBuddy Light 是一个面向 CodeBuddy CN 的跨平台桌面悬浮状态灯。它使用竖排红绿灯显示当前对话状态，并在系统托盘保留入口。目前支持 macOS 和 Windows。

| 灯光 | 状态 | 说明 |
| --- | --- | --- |
| 黄灯闪烁 | 处理中 | CodeBuddy 正在处理消息或调用工具 |
| 红灯常亮 | 等待确认 | CodeBuddy 正在等待权限确认或补充信息 |
| 绿灯 | 已完成 | 回复已经完成，保持点亮直到下一次状态变化 |
| 红灯闪烁 | 执行异常 | 工具调用或响应失败 |
| 红黄绿依次点亮 | 空闲 | 当前没有活跃对话 |

## 工作原理

```text
CodeBuddy Hooks
  -> ~/.codebuddy-light/sessions/<session-id>.json
  -> Tauri Rust 后端聚合多会话状态
  -> 悬浮红绿灯与系统托盘图标
```

多个对话同时存在时，按 `异常 > 等待确认 > 处理中 > 已完成 > 空闲` 的优先级显示。

CodeBuddy 当前没有单独的“普通回复正在等待用户选择” Hook。应用会优先使用工具确认信号，并在 `Stop` 时保守检查回复末尾是否明显在提问或列出选项。

## 开发运行

需要 Node.js、pnpm 和 Rust 工具链。Windows 端的 CodeBuddy Hook 使用系统自带 PowerShell，无需额外安装 Python。

```bash
pnpm install
pnpm tauri:dev
```

启动后，应用会检查 Hooks 是否完整。仅在 Hooks 缺失或需要更新时，悬浮面板才会显示 **安装 Hooks** 按钮。安装器会把 Hook 脚本复制到 `~/.codebuddy-light/hooks/`，并合并写入 `~/.codebuddy/settings.json`。

CodeBuddy 会在会话启动时读取 Hooks 快照。安装或更新 Hooks 后，请新建一个 CodeBuddy 会话再验证灯光状态。

也可以从托盘菜单选择 **安装 CodeBuddy Hooks**。

托盘菜单提供 **开机自启动** 和 **清除会话记录** 选项。悬浮面板会列出当前会话及其最近状态，支持删除单条记录或清除全部记录。

CodeBuddy Light 会监听 CodeBuddy 的本次运行周期。观察到 CodeBuddy 退出后，本轮监听到的会话记录会自动移除；异常残留的记录也会按状态定期回收。

## 本地模拟

不启动 CodeBuddy 也可以切换 UI 状态：

```bash
python3 scripts/simulate.py working
python3 scripts/simulate.py waiting
python3 scripts/simulate.py completed
python3 scripts/simulate.py error
python3 scripts/simulate.py idle
```

## 构建安装包

在对应操作系统上运行：

```bash
pnpm tauri:build
```

macOS 会生成应用包，Windows 会生成 Windows 安装包。仓库的 GitHub Actions 会在 macOS 与 Windows Runner 上分别执行构建检查和 Hook smoke test。

Windows 普通用户请优先使用安装版。安装版内嵌 WebView2 Evergreen Bootstrapper，会在系统缺少 WebView2 Runtime 时引导联网安装。便携版不会自动安装 WebView2，仅适用于系统已经具备 WebView2 Runtime 的电脑。

## 当前阶段

这是首个可运行版本，已经实现 macOS 与 Windows 的 Hook 安装路径和开机启动选项。后续会补充声音提醒和发布安装包。

## License

[MIT](LICENSE)
