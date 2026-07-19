#!/usr/bin/env python3
"""
最小 HTTP 服务：
- GET /api/status  -> 返回当前所有任务状态的 JSON
- GET /             -> 托管正式前端 frontend/index.html

用法：
    python server.py            # 默认监听 0.0.0.0:8787
    python server.py --port 9000

局域网内的其他设备想看这个面板，直接访问 http://<这台机器IP>:8787 即可，
不需要额外部署，因为你本来就在自己的 WireGuard 网络里。
"""

import argparse
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from status_store import get_all

PROJECT_ROOT = Path(__file__).resolve().parents[2]
DASHBOARD_HTML = PROJECT_ROOT / "frontend" / "index.html"


class Handler(BaseHTTPRequestHandler):
    def log_message(self, format, *args):
        pass  # 静默默认日志，太吵

    def do_GET(self):
        if self.path == "/api/status":
            data = get_all()
            body = json.dumps(data, ensure_ascii=False).encode("utf-8")
            self.send_response(200)
            self.send_header("Content-Type", "application/json; charset=utf-8")
            self.send_header("Access-Control-Allow-Origin", "*")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        elif self.path == "/" or self.path == "/dashboard.html":
            if DASHBOARD_HTML.exists():
                body = DASHBOARD_HTML.read_bytes()
                self.send_response(200)
                self.send_header("Content-Type", "text/html; charset=utf-8")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)
            else:
                self.send_response(404)
                self.end_headers()
                self.wfile.write(b"frontend/index.html not found")
        else:
            self.send_response(404)
            self.end_headers()


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--port", type=int, default=8787)
    args = p.parse_args()

    server = ThreadingHTTPServer(("0.0.0.0", args.port), Handler)
    print(f"控制台已启动: http://0.0.0.0:{args.port}  (状态接口: /api/status)")
    server.serve_forever()


if __name__ == "__main__":
    main()
