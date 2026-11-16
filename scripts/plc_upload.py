#!/usr/bin/env python3
"""EnergySentinel PLC 沙盘 —— OpenPLC 程序上传/编译/启动 一键脚本
用法: python3 plc_upload.py [st文件路径]
流程: /login → /upload-program → /upload-program-action → /compile-program → 轮询日志 → /start_plc
默认账密 openplc/openplc（首次登录后建议改）。
"""
import re
import sys
import time
import http.cookiejar
import urllib.request
import urllib.parse
import uuid

BASE = "http://127.0.0.1:8025"
USER = "openplc"
PASS = "openplc"
ST_PATH = sys.argv[1] if len(sys.argv) > 1 else "plc/energy_workshop_plc.st"


def main():
    st = open(ST_PATH, "rb").read()
    cj = http.cookiejar.CookieJar()
    op = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(cj))

    def call(path, data=None, files=None, raw=False):
        body = None
        ctype = None
        if files:
            boundary = uuid.uuid4().hex
            chunks = []
            for field, (fname, content) in files.items():
                chunks.append(f"--{boundary}\r\nContent-Disposition: form-data; name=\"{field}\"; filename=\"{fname}\"\r\nContent-Type: application/octet-stream\r\n\r\n".encode() + content + b"\r\n")
            chunks.append(f"--{boundary}--\r\n".encode())
            body = b"".join(chunks)
            ctype = f"multipart/form-data; boundary={boundary}"
        elif data is not None:
            body = urllib.parse.urlencode(data).encode()
            ctype = "application/x-www-form-urlencoded"
        req = urllib.request.Request(BASE + path, data=body)
        if ctype:
            req.add_header("Content-Type", ctype)
        resp = op.open(req, timeout=30)
        payload = resp.read()
        return payload.decode("utf-8", "replace") if raw else payload

    print("[1/5] 登录", end=" ")
    call("/login", data={"username": USER, "password": PASS, "submit": "Send"})
    print("ok")

    print("[2/5] 上传 ST", end=" ")
    html = call("/upload-program", files={"file": (ST_PATH.split("/")[-1], st)}, raw=True)
    m = re.search(r"<input type='hidden' value='([^']+)' id='prog_file'", html)
    if not m:
        print("\n上传失败：", html[:500])
        sys.exit(1)
    fname = m.group(1)
    print(f"→ {fname}")

    print("[3/5] 登记并触发编译", end=" ")
    call("/upload-program-action", data={
        "prog_name": "energy_workshop_plc",
        "prog_descr": "EnergySentinel workshop sandbox: feeder-im + ws-pump PI (M1)",
        "prog_file": fname,
        "epoch_time": str(int(time.time())),
    })
    call(f"/compile-program?file={fname}")
    print("ok")

    print("[4/5] 等待编译完成", end="")
    for i in range(40):
        time.sleep(3)
        logs = call("/compilation-logs", raw=True)
        if "Compilation finished with errors" in logs:
            print("\n编译报错：\n", logs[-2000:])
            sys.exit(1)
        if "Compilation finished" in logs:
            print(" → 完成")
            break
        print(".", end="", flush=True)
    else:
        print("\n编译超时：\n", call("/compilation-logs", raw=True)[-2000:])
        sys.exit(1)

    print("[5/5] 启动 PLC", end=" ")
    call("/start_plc")
    time.sleep(2)
    logs = call("/runtime_logs", raw=True)
    tail = logs.strip().splitlines()[-1] if logs.strip() else ""
    print(f"→ {tail}")
    print("OK —— Modbus TCP 从站在容器 :502（宿主 5020），Web 在宿主 :8025")


if __name__ == "__main__":
    main()
