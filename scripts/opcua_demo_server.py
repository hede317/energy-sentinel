#!/usr/bin/env python3
"""零硬件演示用 OPC UA 服务器：模拟 PLC 暴露两个标签（管网压力/流量）。
  pip3 install asyncua && python3 opcua_demo_server.py
然后用 opcua_bridge.py 把 ns=2;s=WS.Pressure / ns=2;s=WS.Flow 桥进 EnergySentinel。
"""
import asyncio
import math
import time

from asyncua import Server, ua


async def main():
    srv = Server()
    await srv.init()
    srv.set_endpoint("opc.tcp://0.0.0.0:4840/energy/")
    srv.set_server_name("EnergySentinel Demo PLC")
    idx = await srv.register_namespace("WS")

    pressure = await srv.nodes.objects.add_variable(ua.NodeId("WS.Pressure", idx), ua.QualifiedName("WS.Pressure", idx), 0.42)
    flow = await srv.nodes.objects.add_variable(ua.NodeId("WS.Flow", idx), ua.QualifiedName("WS.Flow", idx), 44.0)
    await pressure.set_writable()
    await flow.set_writable()

    await srv.start()
    print("OPC UA demo PLC on opc.tcp://0.0.0.0:4840/energy/  (ns=2;s=WS.Pressure / WS.Flow)")
    try:
        t0 = time.time()
        while True:
            t = time.time() - t0
            # 压力：0.42 基线 + 慢波 + 偶发用水扰动（每第3分钟压低 0.08）
            pv = 0.42 + 0.03 * math.sin(t / 20) - (0.08 if int(t // 60) % 3 == 2 else 0)
            await pressure.write_value(ua.Variant(round(pv, 3), ua.VariantType.Double))
            await flow.write_value(ua.Variant(round(44 + 6 * math.sin(t / 15), 1), ua.VariantType.Double))
            await asyncio.sleep(1)
    finally:
        await srv.stop()


if __name__ == "__main__":
    asyncio.run(main())
