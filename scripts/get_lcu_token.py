"""
一键获取 LCU 认证信息（需要管理员权限）
双击运行，或在管理员 cmd 中执行：python scripts/get_lcu_token.py
"""
import subprocess
import json
import os
import sys

OUTPUT = r"F:\tft-bot\artifacts\lcu-auth.json"

def main():
    # Check admin
    try:
        import ctypes
        if not ctypes.windll.shell32.IsUserAnAdmin():
            print("需要管理员权限！右键以管理员身份运行。")
            # Auto-elevate
            ctypes.windll.shell32.ShellExecuteW(
                None, "runas", sys.executable, f'"{__file__}"', None, 1
            )
            return
    except Exception:
        pass

    # Read LeagueClientUx command line via wmic
    try:
        result = subprocess.run(
            ["wmic", "process", "where", "name='LeagueClientUx.exe'", "get", "commandline", "/format:list"],
            capture_output=True, text=True, timeout=10
        )
        output = result.stdout
    except Exception as e:
        print(f"wmic 失败: {e}")
        return

    # Parse
    import re
    port_match = re.search(r'--app-port=(\d+)', output)
    token_match = re.search(r'--remoting-auth-token=(.+?)(?=\s--|\s*["\n\r])', output)

    if not port_match or not token_match:
        print(f"未找到认证信息。wmic 输出:\n{output[:500]}")
        return

    port = int(port_match.group(1))
    token = token_match.group(1).strip()

    os.makedirs(os.path.dirname(OUTPUT), exist_ok=True)
    data = {"port": port, "token": token}
    with open(OUTPUT, "w", encoding="utf-8") as f:
        json.dump(data, f)

    print(f"OK! port={port} token={token[:8]}...")
    print(f"保存到: {OUTPUT}")

    # Verify by querying LCU
    import urllib.request, ssl
    ctx = ssl.create_default_context()
    ctx.check_hostname = False
    ctx.verify_mode = ssl.CERT_NONE
    import base64
    cred = base64.b64encode(f"riot:{token}".encode()).decode()
    try:
        req = urllib.request.Request(f"https://127.0.0.1:{port}/lol-gameflow/v1/gameflow-phase")
        req.add_header("Authorization", f"Basic {cred}")
        resp = urllib.request.urlopen(req, timeout=3, context=ctx)
        phase = resp.read().decode().strip('"')
        print(f"LCU 验证成功！当前阶段: {phase}")
    except Exception as e:
        print(f"LCU 验证失败: {e}")

if __name__ == "__main__":
    main()
