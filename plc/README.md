# PLC 沙盘 —— OpenPLC v3 × EnergySentinel（M1）

feeder-im（2# 注塑机回路）+ ws-pump（恒压供水 PI）两台设备的**实测源**来自本机 OpenPLC 软 PLC。
其余 6 台保持 sim 演示源。双源架构（诚实/严实）见 `migrations/schema.sql` v0.7.0 注释。

## 拓扑

```
plc/energy_workshop_plc.st（车间配电逻辑：负载曲线/过载跳闸闩锁/PI 恒压供水）
        │ scripts/plc_upload.py（登录→上传→编译→启动，一键烧录）
        ▼
docker energy-openplc（OpenPLC v3，镜像 energy-openplc，卷 openplc_persist）
        │  Web :8025（容器 8080）    Modbus TCP 从站 :5020（容器 502，slave 1）
        ▼  保持寄存器 HR = ST 里 %QW 直址（1:1）
EnergySentinel :8018（cargo build --release --features rtu）
        │  src/modbus.rs 轮询器：持久连接、按 metrics.reg_addr 点表读 FC03
        ▼
readings/alarms 盖章 source='rtu' → 前端三态开关（全部/⚡实测/🔬演示）
```

运维命令：
- 烧程序：`python3 scripts/plc_upload.py`（默认 `plc/energy_workshop_plc.st`）
- 读寄存器：`python3 scripts/plc_write_hr.py --read 10 8`
- 写命令（遥调）：`python3 scripts/plc_write_hr.py 60 1`（HR60 bit0 = 过载演练 30s）
- feeder-im 复位跳闸：`python3 scripts/plc_write_hr.py 60 8`
- 切设备回演示：`UPDATE devices SET driver='sim' WHERE id='feeder-im';`（≈1 分钟内生效）

## matiec 三坑（OpenPLC v3 ST 编译器）

1. **直址变量只能写在 `CONFIGURATION.VAR_GLOBAL`**：`VAR / VAR_INPUT / VAR_OUTPUT` 里
   `AT %QW10` 一律报 `invalid variable(s) declaration`；程序体用 `VAR_EXTERNAL` 引用。
2. **位运算只作用于 ANY_BIT**：`AND/OR/SHR` 的操作数必须是 WORD/BYTE/DWORD/LWORD，
   `UINT AND 16#FF` 报类型不匹配 → 寄存器全部声明 WORD，数值计算 `WORD_TO_REAL/UINT_TO_WORD` 转换。
3. **数值运算类型要显式对齐**：`UINT / REAL` 除法报 mismatch → 用 `WORD_TO_REAL` 再除；
   十六进制字面量用类型化写法 `WORD#16#FFFE`。

## OpenPLC Modbus 寄存器布局坑

- `%MW` 在 Modbus 侧**偏移 +1024**（`webserver/core/modbus.cpp MIN_16B_RANGE`）：
  HR 0..1023 = %QW（int_output），HR 1024+ = %MW（int_memory）。
- 本沙盘全部用 **%QW → HR 直映**，EnergySentinel 点表 `metrics.reg_addr` 无需任何偏移。
- Modbus TCP 从站**单客户端**：后来的连接踢掉先前的。EnergySentinel 轮询器持有持久连接
  （`App.plc` 槽位）；外部脚本（plc_write_hr.py / AI）偶发接入最多造成一拍 OFFLINE，下拍自愈。

## 编译纪律（踩过的坑）

- **生产二进制必须 `cargo build --release --features rtu` 编译**（launchd 跑
  `target/release/energy-sentinel`，最后一次构建决定形态）。先 build 无 feature 再 build rtu
  会把无 feature 二进制留在位 → 全部 rtu 设备诚实 OFFLINE（这是架构行为，不是故障）。

## 演示剧本（面试/视频三窗口同框）

1. 打开 EnergySentinel :8018（⌜演示⌝徽章 + ⚡实测徽章并存）+ OpenPLC Web :8025 + 终端
2. `python3 scripts/plc_write_hr.py 60 1` → PLC 内 +40kW 演练
3. 若撞上合模段：95kW > 81kW → **PLC 自保护跳闸闩锁**（100ms 级，比 SCADA 快）；
   SCADA 告警引擎的 15s 持续条件正确地不把瞬态误报 → 讲"保护与监控分工"
4. `python3 scripts/plc_write_hr.py 57 180` → 泵需求扰动 → 压力跌破 0.38 →
   **告警触发（source=rtu）**；恢复 `57 0` → PI 回稳 → **告警自动消警**
5. 能耗报表：演示 kWh 与实测 kWh 分列，绝不混算

## M2 待办

- EnergySentinel 遥调 API（/api/sim/cmd 同款入口，rtu 设备走 Modbus 写命令寄存器，审计+返校）
- feeder-im 的 HR17 状态字映射为 breaker/告警（跳闸事件进 SOE）
- 其余设备逐步迁移（meter-main 混合加总口径先设计）
- modbus-mcp 接 AI：Agent 读寄存器诊断 + 写命令寄存器遥调
