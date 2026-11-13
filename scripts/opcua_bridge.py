#!/usr/bin/env python3
"""OPC UA → EnergySentinel 桥：把 PLC/OPC UA 服务器的节点值轮询推入开放 API ingest。

用法（先把 EnergySentinel 的 token 填进来）：
  pip3 install asyncua            # 或建 venv
  python3 opcua_bridge.py \
    --endpoint opc.tcp://127.0.0.1:4840 \
    --url http://127.0.0.1:8018 \
    --token <X-API-Token> \
    --map "ns=2;s=WS.Pressure|opcua-plc|pressure" \
    --map "ns=2;s=WS.Flow|opcua-plc|flow" \
    --interval 2

映射格式： <NodeId>|<device>|<metric>   （NodeId 用 asyncua 语法，如 ns=2;s=Tag1 或 ns=2;i=1001）
配合 opcua_demo_server.py 可零硬件跑通「PLC(OPC UA) → 数据库 → Web 看板」全链演示。
"""
import argparse
import asyncio
import json
import urllib.request


async def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--endpoint", required=True, help="opc.tcp://host:4840")
    ap.add_argument("--url", default="http://127.0.0.1:8018")
    ap.add_argument("--token", required=True)
    ap.add_argument("--map", action="append", required=True, help="NodeId|device|metric")
    ap.add_argument("--interval", type=float, default=2.0)
    a = ap.parse_args()

    from asyncua import Client

    maps = []
    for m in a.map:
        nid, dev, metric = m.split("|")
        maps.append((nid, dev, metric))

    def ingest(dev, metric, value):
        body = json.dumps({"device": dev, "metric": metric, "value": value}).encode()
        req = urllib.request.Request(
            a.url + "/api/v1/open/ingest", data=body,
            headers={"Content-Type": "application/json", "X-API-Token": a.token})
        try:
            with urllib.request.urlopen(req, timeout=5) as r:
                r.read()
        except Exception as e:  # noqa: BLE001
            print("ingest error:", e)

    async with Client(url=a.endpoint) as client:
        print("connected:", a.endpoint)
        while True:
            for nid, dev, metric in maps:
                try:
                    v = await client.get_node(nid).read_value()
                    ingest(dev, metric, float(v))
                    print(f"{nid} -> {dev}/{metric} = {v}")
                except Exception as e:  # noqa: BLE001
                    print("read error:", nid, e)
            await asyncio.sleep(a.interval)


if __name__ == "__main__":
    asyncio.run(main())
