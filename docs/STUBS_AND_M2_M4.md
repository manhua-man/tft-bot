# 什么是「骨架 + Stub」（M2–M4 白话说明）

## 一句话

**骨架** = 目录、类型、函数接口、命令行都已经写好，编译能通过，文档里也有。  
**Stub** = 关键步骤是**假实现/空实现**，不会在真实游戏窗口上截屏、识字、点鼠标。

所以：**程序结构像成品，但还不能当真机 bot 用。**

## 三个 Stub 分别缺什么

| 组件 | 文件里叫什么 | 现在做什么 | 真机需要做什么 |
|------|----------------|------------|----------------|
| 找窗口 | `StubWindowDiscovery` | 直接报错「找不到窗口」 | 枚举 Windows 窗口，锁定英雄联盟/TFT 客户端 |
| 识字 | `StubOcr` | 返回空文字 | 截屏 + OCR（或模板匹配）读出商店五格、金币等 |
| 点按 | `StubInput` | 什么都不点 | 用 Win32/驱动 在正确坐标点击、拖拽 |

`runtime-observe`、`agent-cli run-bot`、`RealEnv` 目前默认注入的就是上面三个 Stub，所以跑起来**不会控制你的游戏**，只是在走流程、写 JSON、测 ONNX 推理链路。

## M2 / M3 / M4 各指什么

| 阶段 | 你要的能力 | 当前状态 |
|------|------------|----------|
| **M2** | 真窗 + 真 OCR + 拿牌能验证（金币/商店变化） | 有 `tft-executor` 模块和 `executor-probe`，但背后是 Stub |
| **M3** | `RealEnv` + `run-bot` 在**真实商店阶段**用模型选动作 | `RealEnv` 代码在，输入观测仍来自 Stub |
| **M4** | 整局更多动作 + 急停 redline + 稀疏终局奖励 | Sim 里较完整；真机全循环未接 |

**M1（已完成）** 只在 **SimEnv** 里 RL，不碰真实客户端，所以不需要 Stub 换成真实现。

## 和 M1 的对比（帮助记忆）

```
M1:  Python PPO  ←→  agent-cli sim-env  ←→  SimEnv（纯内存仿真）     ✅ 已验收
M2+: executor-probe / run-bot  ←→  tft-executor（win+ocr+input）  ✅ 代码已接；需本机 SOP 验收
```

## 本机验收（国服客户端）

```bash
npm run m2:build
npm run m2:preflight
# 游戏在商店阶段：
cargo run -p executor-probe --release --features win_window,ocr_winrt,input_sim -- read-shop
cargo run -p executor-probe --release --features win_window,ocr_winrt,input_sim -- buy --slot 2
```

若 OCR 全空：在 Windows 设置中安装 **中文简体 OCR 语言包**，并确认 `executor-probe preflight` 里 OCR 有效槽位 > 0。

未加 `--features` 编译时仍会使用 Stub，表现会像「骨架」。
