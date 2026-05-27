# 外部参考项目

按**用途**分类，便于 M2 真机（OCR/脚本）和 Sim RL（M1）分开借鉴。链接以 GitHub 搜索与社区常见仓库为准；接入前请自行看 License 与 ToS 风险。

---

## A. OCR / 视觉 + 脚本操作游戏（和 M2–M3 最相关）

这类项目**不强调 RL**，但和 tft-bot 的「找窗 → 截图 → 识别 → 点击 → 校验」同构。

| 项目 | 链接 | 技术栈 | 可借鉴点 |
|------|------|--------|----------|
| **Granblue Automation (PyAutoGUI)** | https://github.com/steve1316/granblue-automation-pyautogui | Python, CV, PyAutoGUI | 工作流编排、多模板、状态机式自动化 |
| **Granblue Automation (Android)** | https://github.com/steve1316/granblue-automation-android | Kotlin, MediaProjection | 移动端截屏 + 识别管线（若做模拟器可参考） |
| **XO-MarketBot (Crossout)** | https://github.com/adibarra/XO-MarketBot | OCR + 模板匹配 | 游戏内 UI 读数 + 自动交易循环 |
| **Whiteout Survival bot** | https://github.com/AminulIslamSifat/wos | 多账号、调度、OCR | 任务调度、角色轮换、屏上交互 |
| **Typing Game Automation** | https://github.com/jiraroj-wir/Typing-Game-Automation-Bot | Tesseract + 高速输入 | 小区域 OCR + 低延迟反馈 |
| **Pokemon Bot With OCR** | https://github.com/newton-shahi/Pokemon-Bot-With-OCR | OCR 驱动网页游戏 | 简单「读字 → 决策 → 点击」闭环 |
| **roll_bot** | https://github.com/Rdnaskello/roll_bot | OCR + 模板 | 徽章/数值类 UI 读取 |
| **Monopoly GO bot** | https://github.com/ethanriverpage/monopolygobot | Python + 模拟器 | 模拟器窗口自动化路径 |

**本地规格（若你仍保留拷贝）**：JinChanChanTool（JCCT）— 五格商店、纠错表、拿牌；不依赖 F:/TFT 仓库存在。

---

## B. PyAutoGUI / OpenCV 轻量游戏 bot（结构简单）

| 项目 | 链接 | 可借鉴点 |
|------|------|----------|
| **sushigoroundbot** | https://github.com/asweigart/sushigoroundbot | 经典 PyAutoGUI 小游戏 bot，代码短 |
| **fruit-box-bot** | https://github.com/kevinychen/fruit-box-bot | 屏幕识别 + 完美分数策略 |
| **2048_bot** | https://github.com/gil9red/2048_bot | 离散状态 + 自动按键 |
| **t_rex_bot** | https://github.com/arsho/t_rex_bot | 极简图像判定 |
| **Python-Game-Bot** | https://github.com/Tanmoy-Mondal-07/Python-Game-Bot | OpenCV + PyAutoGUI +「类人」延迟 |
| **Garden-Game-QoL** | https://github.com/CyberSphinxxx/Garden-Game-QoL | OpenCV + pynput + pyautogui |
| **Metin2 Farming Bot** | https://github.com/nicoladarius/Metin2-Automatic-Farming-Bot | 长期运行的挂机脚本结构 |

Rust 侧若不想用 Python 驱动，可把上述项目的**流程图**迁到 `tft-executor`，识别/输入用 Rust crate 重写。

---

## C. 更复杂客户端 /「AI 玩游戏」（偏研究与工程）

| 项目 | 链接 | 可借鉴点 |
|------|------|----------|
| **EVE-Online-Bot** | https://github.com/darkmatter2222/EVE-Online-Bot | 长周期 MMO 自动化 + ML 叙事；状态多 |
| **Soccer Stars Game Bot** | https://github.com/parissashahabi/Soccer-Stars-Game-Bot | 对战类实时游戏的感知-动作环 |

---

## D. 强化学习 + 游戏仿真（和 M1 / Sim 最相关）

| 项目 | 链接 | 可借鉴点 |
|------|------|----------|
| **rusted-spire** | https://github.com/lhy-loveworld/rusted-spire | Rust 无头 sim + RL |
| **STS-RL** | https://github.com/San-sin-sun/STS-RL | BC + PPO，固定 seed 评测 |
| **SuperAutoPetsAI** | https://github.com/esterRozen/SuperAutoPetsAI | 自走棋式 MDP（比 TFT 简单） |
| **sap-rl** | https://github.com/pjd713/sap-rl | SAP 的 RL 管线 |

详见历史分类；TFT 公开 RL 仓库极少，M1 以本仓 `SimEnv` 为主。

---

## E. 工具库（实现 M2 时常用）

| 类型 | 示例 | 说明 |
|------|------|------|
| 截图 | `screenshots`, Windows GDI | 绑定 HWND 或全屏裁切 |
| OCR | Tesseract, Windows OCR API, PaddleOCR | 商店短文本 + 纠错表 |
| 模板 | OpenCV matchTemplate | 图标、阶段 UI |
| 输入 | Win32 `SendInput`, `enigo` | 与 DPI/分辨率缩放绑定 |
| 模拟器 | ADB, BlueStacks 宏 | 手游/模拟器类自动化 |

---

## 建议阅读顺序（面向当前 tft-bot）

1. **A 类任选一个**（推荐 Granblue 或 XO-MarketBot）— 理解 OCR/脚本状态机。  
2. **JCCT 文档/本地拷贝** — 五格商店与拿牌验收标准。  
3. **D 类 STS-RL 或 rusted-spire** — 固定 seed 评测与 Rust sim。  
4. 本仓 [STUBS_AND_M2_M4.md](STUBS_AND_M2_M4.md) — 分清 Sim 已完成 vs 真机 Stub。

## 维护说明

- 旧文档中的 **F:/TFT** 已删除，不再作为迁移源；新能力均在 **F:/tft-bot** 内实现。  
- 若你发现高质量「云顶/自走棋 OCR bot」仓库，可在本文件追加一行并注明 MDP 是否公开。
