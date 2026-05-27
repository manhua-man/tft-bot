# LCU（国服）

## 认证

- `LeagueClient\lockfile` 常为空，**不要依赖**。
- 从 `Game/Logs/LeagueClient Logs/*_LeagueClientUx.log` 解析 `--app-port` 与 `--remoting-auth-token`（`probe_lcu` / `lcu-probe` 会自动写 `artifacts/lcu-auth.json`）。
- 也可：`python scripts/extract_lcu_auth.py`

## 命令

```bash
lcu-probe
lcu-probe --accept --dry-run
lcu-probe --accept --i-know   # 会真的接受对局
```

`TFT_META_MODE=lcu`：大厅 FSM（建房/排队/接受）。日志无 token 时用 `manual`。

## 2999

```bash
curl -k https://127.0.0.1:2999/liveclientdata/allgamedata
```

局内买棋见 [REAL_MACHINE_SOP.md](REAL_MACHINE_SOP.md)。
