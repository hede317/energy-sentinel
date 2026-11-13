# EnergySentinel · 车间配电与能源监控哨兵 / Workshop Power & Energy Monitoring Sentinel

Rust 单二进制：**Modbus/OPC UA 采集 → PostgreSQL → 多语言看板 · 告警 · 工单 · 开放 API**。
当前以仿真运行（工厂级配电场景 + 恒压供水 PLC 教学场景），硬件到位后切真实驱动，系统与数据链路零改动。
分层定位与 L3/L4 接口见 `docs/ISA-95映射与L3L4接口.md`。

## 运行 / Run

- 服务：`http://127.0.0.1:8018`（launchd 常驻 `com.hedeyou.energy-sentinel`，日志 `/tmp/energy-sentinel.log`）
- 数据库：Docker `energy-pg` @ **5436**（库 `energy_sentinel`，energy/<你的密码>）
- 依赖：Docker Desktop 需运行（energy-pg）；Docker 没开时本服务会自动重试等待

```bash
# 手动管理
launchctl kickstart -k gui/$(id -u)/com.hedeyou.energy-sentinel   # 重启
tail -f /tmp/energy-sentinel.log
ES_PORT=8018 ES_DB=postgresql://... ./target/release/energy-sentinel  # 自定义
cargo build --release                # 日常构建
cargo build --release --features rtu # 硬件日：Modbus-RTU 串口驱动（⚠ 规划项未实现，真表到货当天开发）
```

## 功能 / Features

- **总览**：SVG 一次接线图（进线→总表→母排→4 分回路），实时 U/I/P/PF/EPi，通信状态灯、分合闸（点击图上断路器符号）
- **供水实验台**：恒压供水闭环（压力设定 → 变频器调频 → 泵功率∝f³ → 管网压力回读），PI 调节 + 用水高峰扰动 + 压力低告警——汇川 PLC 教学经典场景
- **趋势**：P/U/I/PF/压力/频率/流量曲线，1h/8h/24h，点击接线图任意回路直达
- **能耗报表**：日/月报，**峰/平/谷分时电价**分桶计价（费率可在界面改），CSV 导出
- **告警**：阈值+持续时间+滞回（防抖）+ 通信看门狗（15s 断链告警），状态机 active→acked→closed（自愈自动关闭）
- **工单**：告警一键转工单，处理记录留痕，闭环时间归档
- **演示触发器**：模拟过载/通信故障/用水高峰——3 分钟讲完 IT+OT 全链的演示剧本
- **对接**：出站 Webhook（告警即时推送）+ 开放 API（`X-API-Token`）：最新值/历史/告警查询、**第三方数据推送 ingest**、CSV 导出；**OPC UA 桥**（`scripts/opcua_bridge.py`，配演示服务器零硬件跑通 PLC→OPC UA→看板）
- **7 语**：中文/English/Français/Português/Español/اردو(RTL)/বাংলা，右上角切换

## 架构 / Architecture

```
安科瑞ADL400/正泰DDSU666(RS485) ──USB转485──┐  （rtu 驱动：真表到货日实现，接口已预留）
PLC / OPC UA 服务器 ── scripts/opcua_bridge.py ─┤
第三方平台 POST /api/v1/open/ingest ────────────┼→ collector (2s) → PostgreSQL(5436)
仿真世界（配电负荷曲线 + 恒压供水闭环）──────────┘        │
                                                         ├→ alarm engine（规则在 alarm_rules 表，配置化）
                                                         ├→ SSE 实时推送
                                                         └→ axum Web :8018（单页看板，无前端构建链）
```

## 安全边界 / Security

内网演示定位：内部看板 API 无鉴权（浏览器同源保护，未加 CORS 头，跨站读不到）；**开放 API 走 X-API-Token**，token 不在任何接口明文回显（首启 token 在服务日志，`对接`页凭 token 查看/换发）；Webhook 仅 http(s)。公网部署前须加反向代理鉴权 + HTTPS。

- 表：`devices` / `metrics`（**寄存器映射配置化**：地址+缩放系数在表里，换表不改代码）/ `readings`（31 天滚动）/ `alarm_rules` / `alarms` / `work_orders` / `wo_notes` / `settings`
- 对标产品：FUXA（:8014，组态画面参考）、ThingsPanel（:8016，物模型/告警/工单业务流参考）

## 硬件接入 / Hardware Day

见 `docs/硬件接入-安科瑞正泰与汇川.md`（BOM、接线、寄存器表、driver 切换步骤、安全注意事项）。

## 开放 API 速查

```bash
TOKEN=$(curl -s localhost:8018/api/settings | jq -r .api_token)
curl -H "X-API-Token: $TOKEN" localhost:8018/api/v1/open/latest
curl -X POST -H "X-API-Token: $TOKEN" -H 'Content-Type: application/json' \
     -d '{"device":"ext-temp-01","metric":"temp","value":26.5}' localhost:8018/api/v1/open/ingest
```
