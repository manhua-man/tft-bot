# Hermes Agent vs Claude Code 能力对比

> 基于 Claude-Code-rev 源码分析 + Hermes Agent 实际能力

---

## 1. 总览对比

```
+=====================================================================+
|                    总体对比                                          |
+=====================================================================+
|                        Hermes Agent    Claude Code                   |
|  ─────────────────────────────────────────────────────────           |
|  技术栈                Python          TypeScript/Bun               |
|  UI 框架               CLI/TUI         Ink (React for Terminal)     |
|  核心工具数             16              51+                          |
|  斜杠命令数             0               88                           |
|  技能系统               83 skills       plugin + skills              |
|  代码量                 ~5K lines       ~200K+ lines                 |
|  ─────────────────────────────────────────────────────────           |
|  模型支持               任意 LLM        Claude only                  |
|  平台交付               CLI/TG/Discord/  CLI only (+Desktop)         |
|                         Slack/Web/SMS                                |
|  记忆系统               内置 memory      CLAUDE.md + MemoryTool      |
|  MCP 协议               原生客户端       内置 + Auth                  |
|  插件系统               无 (用技能)      完整 plugin system           |
|  ─────────────────────────────────────────────────────────           |
|  独特能力               多平台交付        TUI React 渲染              |
|                         图像生成          88 slash commands           |
|                         语音合成          原生 Git 集成               |
|                         浏览器自动化      团队协作                    |
|                         定时任务          语音输入                    |
|                         子代理并行        Vim 模式                    |
|                         83 领域技能       SSH 远程执行                |
|                                          LSP 语言服务                |
|                                          Computer Use               |
|                                          Worktree 管理              |
+=====================================================================+
```

---

## 2. 工具对比

```
+=====================================================================+
|                     工具能力矩阵                                     |
+=====================================================================+
|                                                                     |
|  [文件操作]                                                         |
|  Hermes:     read_file / write_file / patch / search_files          |
|  Claude:     FileReadTool / FileWriteTool / FileEditTool            |
|              GlobTool / GrepTool / NotebookEditTool                  |
|  对比:       基本对等。Claude 多了 NotebookEdit (Jupyter)            |
|              Hermes 的 patch 有 9 种模糊匹配策略更灵活              |
|                                                                     |
|  [终端执行]                                                         |
|  Hermes:     terminal (bash, 前台/后台/PTY)                        |
|  Claude:     BashTool / PowerShellTool / REPLTool                   |
|              TerminalCaptureTool                                    |
|  对比:       Claude 多了 PowerShell 和 REPL 直接支持                |
|              Hermes 通过 bash 兼容层也支持 PowerShell 命令           |
|                                                                     |
|  [浏览器]                                                           |
|  Hermes:     browser_* (navigate/click/type/snapshot/vision/        |
|              console/scroll)  — 7 个子工具                          |
|  Claude:     WebBrowserTool / WebFetchTool / WebSearchTool          |
|  对比:       Hermes 浏览器更深 (完整 Playwright 控制)               |
|              Claude 更广 (内置搜索+抓取, 不需外部浏览器)             |
|                                                                     |
|  [图像]                                                             |
|  Hermes:     vision_analyze + image_generate                        |
|  Claude:     无原生图像工具                                         |
|  对比:       Hermes 独有优势                                         |
|                                                                     |
|  [语音]                                                             |
|  Hermes:     tts (文本转语音)                                       |
|  Claude:     voice command (语音输入)                               |
|  对比:       Hermes 有输出, Claude 有输入, 互为补充                  |
|                                                                     |
|  [子代理/任务]                                                      |
|  Hermes:     delegate_task (leaf/orchestrator, 最多 3 并行)         |
|  Claude:     AgentTool + TaskCreate/List/Get/Update/Stop/Output     |
|              + TeamCreate/TeamDelete                                |
|              + LocalAgentTask / RemoteAgentTask                     |
|  对比:       Claude 任务系统远更成熟:                                |
|              - 任务创建/查询/停止/输出分离                           |
|              - 支持远程代理任务                                      |
|              - 支持团队协作                                          |
|              - 有 MonitorTool 监控任务状态                          |
|              Hermes 更简单但够用 (3 并行子代理)                      |
|                                                                     |
|  [定时任务]                                                         |
|  Hermes:     cronjob (创建/管理/暂停/恢复)                         |
|  Claude:     ScheduleCronTool                                       |
|  对比:       基本对等                                                |
|                                                                     |
|  [记忆]                                                             |
|  Hermes:     memory (add/replace/remove, user+memory)               |
|  Claude:     MemoryTool + SessionMemory + teamMemorySync            |
|              + extractMemories 服务                                 |
|  对比:       Claude 记忆系统更完善:                                  |
|              - 自动提取记忆                                          |
|              - 会话记忆                                              |
|              - 团队记忆同步                                          |
|                                                                     |
|  [Git/GitHub]                                                       |
|  Hermes:     通过 terminal 执行 git/gh 命令                        |
|  Claude:     内置 git utils + GitHub utils                          |
|              branch command / commit-push-pr                        |
|              pr_comments / issue / review                           |
|              autofix-pr / install-github-app                        |
|  对比:       Claude 原生集成更深, 有专门的 PR 评论/Issue 命令       |
|              Hermes 通过 gh CLI 也能完成相同操作                     |
|                                                                     |
|  [MCP]                                                              |
|  Hermes:     native-mcp 客户端                                      |
|  Claude:     MCPTool + McpAuth + ListMcpResources + ReadMcpResource |
|  对比:       Claude 支持 MCP Auth (OAuth), Hermes 不支持            |
|                                                                     |
|  [Plan 模式]                                                        |
|  Hermes:     plan skill (写 markdown 到 .hermes/plans/)             |
|  Claude:     EnterPlanModeTool / ExitPlanModeTool                   |
|              VerifyPlanExecutionTool                                |
|  对比:       Claude 原生 plan mode 有验证执行                        |
|              Hermes 通过 skill 实现类似功能                          |
|                                                                     |
+=====================================================================+
```

---

## 3. 独有能力对比

```
+=====================================================================+
|              Hermes 独有 (Claude Code 没有)                         |
+=====================================================================+
|                                                                     |
|  [多平台交付]                                                       |
|  +-- CLI (终端直接交互)                                            |
|  +-- Telegram (机器人)                                              |
|  +-- Discord (机器人 + 频道)                                        |
|  +-- Slack (App)                                                    |
|  +-- Web (HTTP)                                                     |
|  +-- SMS                                                            |
|  +-- Feishu (飞书)                                                  |
|  Claude: 仅 CLI (+ Desktop App)                                    |
|                                                                     |
|  [图像生成]                                                         |
|  +-- image_generate: 文本 → 图片                                    |
|  +-- 支持 OpenAI / xAI / FAL 等后端                                 |
|  +-- 可选 landscape / portrait / square                             |
|  Claude: 无                                                         |
|                                                                     |
|  [视觉分析]                                                         |
|  +-- vision_analyze: 图片 → AI 描述                                 |
|  +-- browser_vision: 截图 → AI 分析                                 |
|  Claude: 无原生视觉工具 (需 Computer Use)                           |
|                                                                     |
|  [语音合成]                                                         |
|  +-- tts: 文本 → 语音 (Edge/OpenAI/xAI/自定义)                     |
|  +-- 多语言支持                                                     |
|  Claude: 仅语音输入, 无输出                                         |
|                                                                     |
|  [83 个领域技能]                                                    |
|  +-- 创意: ASCII艺术/像素/漫画/信息图/音乐/视频/TouchDesigner       |
|  +-- 研究: arXiv/博客监控/预测市场/LLM Wiki                        |
|  +-- 媒体: YouTube/Spotify/GIF/音频分析                             |
|  +-- 游戏: Pokemon 玩家                                             |
|  +-- 智能家居: Philips Hue                                          |
|  +-- MLOps: HF/W&B/llama.cpp/SAM/DSPy                              |
|  +-- 生产力: Gmail/Calendar/Notion/Linear/PP/PDF                    |
|  +-- 邮件: Himalaya CLI                                             |
|  +-- 红队: LLM jailbreak                                           |
|  Claude: Skills 系统较新, 领域覆盖远不如                            |
|                                                                     |
|  [模型灵活性]                                                       |
|  +-- 支持任意 LLM 提供商                                            |
|  +-- 可配置 custom providers                                        |
|  +-- 每个任务可指定不同模型                                         |
|  Claude: 仅 Claude 模型                                             |
|                                                                     |
|  [Python 代码执行]                                                  |
|  +-- execute_code: 完整 Python 脚本执行                             |
|  +-- 可调用所有 Hermes 工具                                         |
|  +-- 50 次工具调用, 5 分钟超时                                      |
|  Claude: REPLTool (TypeScript)                                      |
|                                                                     |
+=====================================================================+

+=====================================================================+
|              Claude Code 独有 (Hermes Agent 没有)                   |
+=====================================================================+
|                                                                     |
|  [完整 TUI]                                                         |
|  +-- Ink (React for Terminal) 渲染引擎                              |
|  +-- 组件化 UI: 语法高亮/Diff视图/进度条/对话气泡                   |
|  +-- 设计系统 (design-system/)                                      |
|  +-- 主题系统 (theme/)                                              |
|  +-- Vim 模式 (vim/)                                                |
|  +-- 键绑定系统 (keybindings/)                                      |
|  Hermes: 纯文本 CLI, 无 TUI                                        |
|                                                                     |
|  [88 个斜杠命令]                                                    |
|  核心: /help /compact /clear /config /model /cost /usage            |
|  Git:  /branch /diff /commit /review /pr_comments                   |
|  任务: /plan /tasks /agents /session /resume                        |
|  高级: /bughunter /thinkback /ultraplan /teleport                   |
|  团队: /memory /skills /share /bridge                               |
|  调试: /doctor /debug-tool-call /heapdump /env                      |
|  社交: /feedback /stickers /good-claude                             |
|  Hermes: 无斜杠命令, 通过自然语言交互                               |
|                                                                     |
|  [团队协作]                                                         |
|  +-- TeamCreateTool / TeamDeleteTool                                |
|  +-- teamMemorySync (团队记忆同步)                                  |
|  +-- bridge (跨会话通信)                                            |
|  +-- SendMessageTool (发送消息给其他会话)                            |
|  Hermes: 无团队功能, 单用户设计                                     |
|                                                                     |
|  [Git 深度集成]                                                     |
|  +-- 内置 git utils (commit/branch/diff)                           |
|  +-- autofix-pr (自动修复 PR)                                       |
|  +-- install-github-app                                             |
|  +-- pr_comments (内联 PR 评论)                                     |
|  +-- issue 命令                                                     |
|  +-- Worktree 管理 (EnterWorktreeTool/ExitWorktreeTool)            |
|  +-- git 感知的文件操作                                             |
|  Hermes: 通过 terminal + gh CLI 间接操作                            |
|                                                                     |
|  [IDE 集成]                                                         |
|  +-- /ide 命令 (VS Code / JetBrains)                               |
|  +-- Chrome 扩展 (/chrome)                                          |
|  +-- Desktop App (/desktop)                                         |
|  +-- Mobile 支持 (/mobile)                                          |
|  +-- SSH 远程 (/remote-setup)                                      |
|  Hermes: 纯 CLI, 无 IDE 集成                                       |
|                                                                     |
|  [Computer Use]                                                     |
|  +-- utils/computerUse/ 模块                                        |
|  +-- 屏幕截图 → AI 分析 → 操作                                     |
|  Hermes: 有 browser_vision 但无桌面级 Computer Use                  |
|                                                                     |
|  [LSP 语言服务]                                                     |
|  +-- LSPTool (Language Server Protocol)                             |
|  +-- 代码补全/定义跳转/引用查找                                     |
|  Hermes: 无 LSP 支持                                                |
|                                                                     |
|  [插件系统]                                                         |
|  +-- plugins/ 目录                                                  |
|  +-- bundled plugins                                                |
|  +-- plugin 命令 (/plugin, /reload-plugins)                        |
|  +-- services/plugins/ 服务                                         |
|  +-- DXT 格式支持                                                   |
|  Hermes: 用 skill 系统替代, 但无运行时插件加载                      |
|                                                                     |
|  [高级任务管理]                                                     |
|  +-- TaskCreate/List/Get/Update/Stop/Output (6 个独立工具)          |
|  +-- LocalAgentTask / RemoteAgentTask                               |
|  +-- InProcessTeammateTask                                          |
|  +-- LocalShellTask / LocalWorkflowTask                             |
|  +-- DreamTask (自动梦境任务)                                       |
|  +-- MonitorMcpTask                                                 |
|  +-- VerifyPlanExecutionTool                                        |
|  Hermes: delegate_task (1 个工具, leaf/orchestrator)                |
|                                                                     |
|  [语音输入]                                                         |
|  +-- voice command (实时语音输入)                                   |
|  +-- 语音识别                                                       |
|  Hermes: 仅 TTS 输出, 无语音输入                                    |
|                                                                     |
|  [其他]                                                             |
|  +-- /compact (上下文压缩)                                          |
|  +-- /thinkback (思考回溯)                                          |
|  +-- /rewind (撤销操作)                                             |
|  +-- /sandbox (沙箱模式)                                            |
|  +-- /teleport (状态跳转)                                           |
|  +-- /effort (推理力度控制)                                         |
|  +-- /fast (快速模式)                                               |
|  +-- proactive (主动建议)                                           |
|  +-- autoDream (自动梦境)                                           |
|  +-- MagicDocs (智能文档)                                           |
|  +-- PromptSuggestion (提示建议)                                    |
|  +-- contextCollapse (上下文折叠)                                   |
|  +-- tips (提示系统)                                                |
|                                                                     |
+=====================================================================+
```

---

## 4. 架构对比图

```
+=====================================================================+
|                     Hermes Agent 架构                                |
+=====================================================================+
|                                                                     |
|  用户                                                               |
|   │                                                                 |
|   ├── CLI ◄────────────────────────── 直接终端交互                  |
|   ├── Telegram ◄───────────────────── 移动端机器人                  |
|   ├── Discord ◄────────────────────── 社区机器人                    |
|   └── Web ◄────────────────────────── HTTP API                     |
|        │                                                            |
|        v                                                            |
|  +────────────────+                                                |
|  │  Gateway       │ ◄── 消息路由层 (多平台统一入口)                 |
|  +───────┬────────+                                                |
|          │                                                         |
|          v                                                         |
|  +────────────────+     +────────────+     +────────────+          |
|  │  推理引擎       │────>│  记忆系统   │────>│  技能系统   |          |
|  │  (任意 LLM)    │     │  (持久化)   │     │  (83个)    |          |
|  +───────┬────────+     +────────────+     +────────────+          |
|          │                                                         |
|          v                                                         |
|  +────────────────────────────────────────+                        |
|  │  16 工具                                │                        |
|  │  terminal file browser search image    │                        |
|  │  patch delegate execute memory skills  │                        |
|  │  todo cronjob vision tts clarify       │                        |
|  │  session                               │                        |
|  +────────────────────────────────────────+                        |
|                                                                     |
+=====================================================================+

+=====================================================================+
|                     Claude Code 架构                                |
+=====================================================================+
|                                                                     |
|  用户                                                               |
|   │                                                                 |
|   ├── CLI ◄────────────────────────── Ink TUI (React)              |
|   ├── Desktop App ◄────────────────── Electron                     |
|   ├── VS Code ◄────────────────────── IDE 扩展                     |
|   ├── Chrome ◄─────────────────────── 浏览器扩展                    |
|   ├── Mobile ◄─────────────────────── 移动端                       |
|   └── SSH ◄────────────────────────── 远程连接                     |
|        │                                                            |
|        v                                                            |
|  +────────────────────────────────────────────────────────+         |
|  │  Ink 渲染引擎                                          │         |
|  │  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐  │         |
|  │  │ 组件系统  │ │ 主题系统  │ │ Vim 模式  │ │ 键绑定   │  │         |
|  │  └──────────┘ └──────────┘ └──────────┘ └──────────┘  │         |
|  +────────────────────────┬───────────────────────────────+         |
|                           │                                        |
|  +────────────────────────v───────────────────────────────+         |
|  │  88 斜杠命令 + 自然语言                                 │         |
|  │  /help /plan /review /bughunter /thinkback /compact     │         |
|  +────────────────────────┬───────────────────────────────+         |
|                           │                                        |
|  +────────────────────────v───────────────────────────────+         |
|  │  51+ 工具                                               │         |
|  │  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐       │         |
|  │  │ 文件操作     │ │ 终端执行     │ │ Web 工具     │       │         |
|  │  │ 6 tools     │ │ 4 tools     │ │ 3 tools     │       │         |
|  │  └─────────────┘ └─────────────┘ └─────────────┘       │         |
|  │  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐       │         |
|  │  │ 任务系统     │ │ MCP 集成     │ │ Git 工具     │       │         |
|  │  │ 7 tools     │ │ 4 tools     │ │ 内置        │       │         |
|  │  └─────────────┘ └─────────────┘ └─────────────┘       │         |
|  │  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐       │         |
|  │  │ Agent       │ │ Plan        │ │ Skills      │       │         |
|  │  │ 1 tool      │ │ 3 tools     │ │ 1 tool      │       │         |
|  │  └─────────────┘ └─────────────┘ └─────────────┘       │         |
|  +─────────────────────────────────────────────────────────+         |
|                                                                     |
|  +─────────────────────────────────────────────────────────+         |
|  │  服务层                                                  │         |
|  │  analytics / compact / lsp / mcp / memory / plugins     │         |
|  │  skillSearch / SessionMemory / teamMemorySync           │         |
|  │  tips / proactive / autoDream / MagicDocs               │         |
|  +─────────────────────────────────────────────────────────+         |
|                                                                     |
+=====================================================================+
```

---

## 5. 总结

```
+=====================================================================+
|                        定位差异                                      |
+=====================================================================+
|                                                                     |
|  Claude Code = 专业编码助手                                        |
|  ──────────────────────────────────                                 |
|  - 专注软件开发工作流                                               |
|  - 深度 IDE 集成 (VS Code/JetBrains/Chrome)                       |
|  - 完整 TUI 体验 (Ink React 渲染)                                  |
|  - 团队协作 (共享记忆/桥接/团队)                                   |
|  - 原生 Git/GitHub 深度集成                                        |
|  - 88 个斜杠命令覆盖开发全流程                                      |
|  - 仅 Claude 模型                                                  |
|  - 仅 CLI/Desktop 平台                                             |
|                                                                     |
|  Hermes Agent = 通用 AI 助手                                       |
|  ──────────────────────────────────                                 |
|  - 多平台交付 (CLI/TG/Discord/Web/SMS)                            |
|  - 多模型支持 (任意 LLM 提供商)                                    |
|  - 83 个领域技能 (创意/研究/媒体/游戏/智能家居/MLOps)              |
|  - 图像生成 + 语音合成                                             |
|  - 浏览器自动化 (Playwright)                                       |
|  - 定时任务系统                                                    |
|  - 子代理并行 (最多 3 个)                                          |
|  - Python 代码执行                                                 |
|  - 无 TUI, 纯文本 CLI                                              |
|  - 无团队协作                                                      |
|  - 无 IDE 集成                                                     |
|                                                                     |
|  ═══════════════════════════════════════════════════                |
|                                                                     |
|  如果你需要:                                                        |
|    → 写代码/PR/Code Review → Claude Code 更好                     |
|    → 多平台通知/自动化     → Hermes Agent 更好                     |
|    → 图像/音频/媒体生成    → Hermes Agent 独有                     |
|    → 团队协作              → Claude Code 独有                      |
|    → 用非 Claude 模型      → Hermes Agent 独有                     |
|    → 最佳 TUI 体验         → Claude Code 独有                      |
|    → 游戏/智能家居/研究    → Hermes Agent 独有                     |
|                                                                     |
|  两者可以互补使用: Hermes Agent 委派编码任务给 Claude Code          |
|                                                                     |
+=====================================================================+
```
