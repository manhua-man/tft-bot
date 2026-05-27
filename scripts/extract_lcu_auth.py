"""
从 LeagueClient 日志中提取 LCU 认证信息（无需管理员权限）
搜索最新的 LeagueClientUx.log 文件，提取 --app-port 和 --remoting-auth-token
"""
import os
import re
import json
import glob

LOG_DIR = r"G:\WeGameApps\英雄联盟\Game\Logs\LeagueClient Logs"
OUTPUT = r"F:\tft-bot\artifacts\lcu-auth.json"

def main():
    # Find the most recent LeagueClientUx.log
    pattern = os.path.join(LOG_DIR, "*_LeagueClientUx.log")
    files = glob.glob(pattern)
    if not files:
        print(f"FAILED: No LeagueClientUx.log found in {LOG_DIR}")
        return
    
    # Sort by filename (contains timestamp)
    files.sort(reverse=True)
    latest = files[0]
    print(f"Reading: {os.path.basename(latest)}")
    
    # Read the log and extract command line
    with open(latest, "r", encoding="utf-8", errors="ignore") as f:
        content = f.read()
    
    # Extract --app-port and --remoting-auth-token
    port_match = re.search(r'--app-port=(\d+)', content)
    token_match = re.search(r'--remoting-auth-token=([^\s"]+)', content)
    
    if not port_match or not token_match:
        print("FAILED: Could not find --app-port or --remoting-auth-token in log")
        return
    
    port = int(port_match.group(1))
    token = token_match.group(1)
    
    os.makedirs(os.path.dirname(OUTPUT), exist_ok=True)
    data = {"port": port, "token": token}
    with open(OUTPUT, "w", encoding="utf-8") as f:
        json.dump(data, f)
    
    print(f"OK! port={port} token={token[:8]}...")
    print(f"Saved to: {OUTPUT}")

if __name__ == "__main__":
    main()
