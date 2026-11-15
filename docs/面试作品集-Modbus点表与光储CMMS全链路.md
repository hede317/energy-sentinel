# 面试作品集 · Modbus 点表与光储 CMMS 全链路

> **项目**：energy-sentinel（EnergySentinel · 车间配电与能源监控哨兵）
> **作者**：何德友
> **目标岗位**：工业物联网 IIoT / 光伏储能 EMS / SCADA–MES–ERP 集成
> **技术栈**：Rust（axum + tokio，单二进制）、PostgreSQL（Docker `energy-pg` @ 5436）、Modbus-RTU / Modbus-TCP / OPC UA、零前端构建链的单页看板
> **一句话定位**：我做的系统卡在 **L2 SCADA 采集监控** 与 **L3 MES/能管业务** 之间——南向吃 L1/L2 的 Modbus/OPC UA 数据，北向用开放 API + Webhook + CSV 把能耗与工单喂给 L4 ERP，正好是 IT/OT 边界上最缺人的位置。

---

## 0. 一页速览（面试官 30 秒版）

| 维度 | 内容 |
|---|---|
| 交付形态 | Rust 单二进制，`cargo build --release` 直接跑，默认 `http://0.0.0.0:8018` |
| 数据库 | PostgreSQL `energy_sentinel` @ **127.0.0.1:5436**；schema 幂等，启动时 `batch_execute` 自动建表/播种 |
| 采集周期 | collector **2s** 一拍；`readings` 滚动 31 天；SSE `/api/stream` 实时推送 |
| 核心对象 | `devices` / `metrics`（**寄存器映射配置化**：地址+缩放+功能码+字节序全在表里，换表不改代码）/ `readings` / `alarm_rules` / `alarms` / `work_orders` / `wo_notes` / `audit_logs` / `settings` |
| v0.6.0 光储扩展 | `pv_ess` 设备（PV-INV / BMS / PCS）+ `comm_logs`（抓帧与排障记录）+ `inspections`（巡检表）+ `defects`（缺陷台账）+ `switching_tickets`（倒闸操作票） |
| 告警学 | **阈值 + 持续时间 + 滞回（防抖）** 三件套；`active → acked → closed` 状态机；**15s 通信看门狗** 断链告警、自愈自动消警 |
| 能管 | **峰/平/谷分时电价** 分桶计价（费率可在界面改），`/api/report/day`、`/api/report/month`、CSV 导出 |
| 集成 | 出站 **Webhook**（`alarm.triggered` / `alarm.closed`）+ **开放 API `X-API-Token` 鉴权** + `/api/v1/open/ingest` 第三方推数 |
| 安全 | 选择—返校—执行全程写 `audit_logs`；内网看板同源保护，开放 API 走令牌，token 不在接口明文回显 |

> **诚实边界（面试加分项，我会主动说）**：当前 `devices.driver` 全部为 `sim`，`--features rtu` 串口驱动是**规划项、接口已预留**（`devices.meta` 的 port/slave/baud + `metrics` 的 reg_addr/scale/fc/dtype/endian/rw 已就位）；OPC UA 路线已可用（`scripts/opcua_bridge.py` 配演示服务器零硬件跑通）。光储点表、CMMS 单据模型、抓帧排障记录都是**已落库的真实 schema 与种子数据**，因此这份手册讲的是"系统先行、硬件后接"的完整工程口径，不是 PPT 架构。

**全文导航**：§1 网络拓扑 → §2 寄存器点表 → §3 报文帧与 CRC16 → §4 断线排错 → §5 四大单据与 ERP/CMMS 映射 → §6 五分钟演示台词。

---

## 1. IT/OT 五层网络拓扑（ISA-95 / Purdue 模型）

### 1.1 五层拓扑总图

```mermaid
flowchart TB
  subgraph L4["L4 企业/经营层 · IT 网 10.10.0.0/16"]
    ERP["ERP 财务总账<br/>NetSuite / ERPNext / 用友<br/>定时拉 /api/v1/open/export/day"]
    CMMS["MES / CMMS<br/>缺陷工单 · 成本归集 · 日报月报"]
  end

  subgraph L35["L3.5 工业防火墙 / DMZ · 北向唯一出口"]
    FW["工业防火墙白名单<br/>仅放行 TCP 8018 北向开放 API<br/>严禁外网直连 VLAN10/20 与 :502"]
  end

  subgraph L3["L3 / L2 监控主站 · VLAN30 192.168.30.0/24"]
    SCADA["SCADA/EMS 监控主站<br/>Rust axum :8018 + PostgreSQL :5436<br/>SSE /api/stream · 看板 · 告警引擎 · 工单"]
    HMI["值班 HMI / 工程师站"]
  end

  subgraph L2["L2 工业环网交换机（三层，VLAN 隔离）"]
    SW["三层交换机<br/>VLAN10 192.168.10.0/24 电力仪表与 PLC 控制专网<br/>VLAN20 192.168.20.0/24 光伏与储能 BMS/PCS 新能源专网<br/>VLAN30 192.168.30.0/24 SCADA/EMS 监控主站"]
  end

  subgraph L15["L1.5 边缘网关 · 协议转换与断网缓存"]
    GW["串口服务器 USR-N540<br/>RTU → TCP 透明转换 · 断网缓存"]
    OPC["OPC UA 桥 :4840<br/>scripts/opcua_bridge.py"]
  end

  subgraph L1["L1 控制/仪表层"]
    METER["安科瑞 ADL400 进线总表/分路表<br/>Slave 0x01~0x05 · RS485 Modbus-RTU"]
    PLC["汇川 PLC Easy521 / 变频器<br/>Slave 10 · 恒压供水闭环"]
    BMS["储能 BMS-01 · PCS-01<br/>Unit 0x01 / 0x02 · Modbus-TCP :502"]
    PV["光伏逆变器 SG110CX<br/>Slave 0x11 · RS485 Modbus-RTU"]
  end

  subgraph L0["L0 现场设备层"]
    DEV["断路器 QF0~QF4 · CT/PT · 母排 · 电机/泵<br/>光伏组件/汇流箱 · 锂电簇/液冷/消防 · 传感器"]
  end

  DEV --> METER & PLC & BMS & PV
  METER -- "RS485 手拉手 9600,8,N,1" --> GW
  PV -- "RS485-Bus2 9600,8,N,1" --> GW
  PLC -- "RS485 / OPC UA" --> GW & OPC
  BMS -- "工业以太网 TCP :502" --> SW
  GW -- "Modbus-TCP" --> SW
  OPC -- "OPC UA :4840" --> SW
  SW --> SCADA
  HMI --> SCADA
  SCADA --> FW
  FW --> ERP & CMMS
```

### 1.2 逐层工程规格（物理介质 / 协议 / 端口 / 子网 / 角色）

| 层 | 名称 | 物理介质 | 协议 | 端口 | VLAN / 子网 | 设备角色 |
|---|---|---|---|---|---|---|
| **L0** | 现场设备层 | 动力电缆、CT/PT 二次线、组件直流线、电池高压线 | 无（电气量/机械位置） | — | 无 | 断路器 QF0~QF4、电流/电压互感器、母排、电机/水泵、光伏组件与汇流箱、锂电簇/液冷/七氟丙烷消防——遥信遥测的源头 |
| **L1** | 控制/仪表层 | RS485 屏蔽双绞线（电表/PLC/逆变器）、工业以太网超五类/六类（BMS/PCS） | Modbus-RTU、Modbus-TCP、OPC UA | RTU 无端口；TCP **502**；OPC UA **4840** | VLAN10 / VLAN20 | ADL400 电表、汇川 Easy521 PLC 与变频器、SG110CX 逆变器、BMS-01、PCS-01——从站/服务器端 |
| **L1.5** | 边缘网关 | RS485 进、以太网出；本地 Flash 缓存 | RTU↔TCP 透明转换、OPC UA 桥 | 透传 :502 / :4840 | 网关双址（现场侧 + 监控侧） | **USR-N540 串口服务器**：RTU→TCP 透明转换，串口参数 9600,8,N,1；断网时本地缓存，恢复后续传，避免丢数 |
| **L2** | 工业环网 | 光纤环网 / 六类线，三层交换机 | VLAN Trunk、IGMP、环网冗余协议 | — | **VLAN10 192.168.10.0/24**（电力仪表与 PLC 控制专网）<br>**VLAN20 192.168.20.0/24**（光伏与储能 BMS/PCS 新能源专网）<br>**VLAN30 192.168.30.0/24**（SCADA/EMS 监控主站） | 三层交换机做广播域隔离与跨 VLAN 受控路由；环网自愈，单断点不丢采集 |
| **L3 / L3.5** | 监控主站 + 工业防火墙/DMZ | 以太网，DMZ 双网口隔离 | HTTP REST / SSE / Webhook / CSV | **Rust :8018**、**PostgreSQL :5436**、Webhook 出站 http(s) | VLAN30 → DMZ | EnergySentinel 主站（采集引擎、告警引擎、看板、工单、开放 API）；防火墙做 IT/OT 边界 |
| **L4** | 企业 IT 网 | 园区网 / 专线 / VPN | REST（开放 API）、CSV 导入、Webhook 消费 | TCP **8018**（唯一北向） | **10.10.0.0/16** | MES/CMMS 消费 Webhook 转缺陷工单；ERP 财务总账定时拉 `/api/v1/open/export/day` 取峰平谷电费与光伏自发自用结算单 |

### 1.3 RS485 总线物理规范（手拉手菊花链）

```
   ┌──────────────── 控制室 / 网关侧 ────────────────┐
   │  120Ω 终端电阻                                   │
   │   ┌──┐                                          │
   └───┤T1├──A+══════════╦══════════╦══════════╦═════╦══════┐
       └──┘  B-──────────╩──────────╩──────────╩─────╩──────┤T2├── 120Ω 终端电阻
            RVSP 2×0.75mm² 屏蔽双绞线                        └──┘
            屏蔽层单端接地（控制室侧，严禁两端接地形成环流）
              │          │          │          │
           ┌──┴──┐    ┌──┴──┐    ┌──┴──┐    ┌──┴──┐
           │0x01 │    │0x02 │    │0x03 │    │0x11 │   ← 从站地址唯一
           │ADL400│   │ADL400│   │ADL400│   │SG110CX│
           │进线  │   │空压机│   │注塑机│   │逆变器 │
           └─────┘    └─────┘    └─────┘    └─────┘

   规约：Modbus-RTU 9600,8,N,1；手拉手（bus），严禁星型；
         A+ 对 A+、B- 对 B-；两端各 1 个 120Ω（全链路 >2 个会过阻尼）；
         同一总线波特率一致、从站地址唯一；总线两端之外不得再挂终端电阻。
```

- 线缆：**RVSP 2×0.75mm² 屏蔽双绞线**（A/B 各一芯 + 编织屏蔽层），线径比常见 2×0.5 更适合作长距离车间干线。
- 终端电阻：**两端各 1 个 120Ω**，与双绞线特性阻抗匹配，抑制长线反射；短距离（<10m）可省，但工程交付一律补齐。
- 屏蔽层：**单端接地**（控制室/网关侧），两端接地会因地电位差形成环流、把共模噪声耦合进 A/B，典型症状就是"偶发 CRC 误码、轮询重试"——对应 §4 与 `defects` 里 `QF-202611-004`。
- 串口参数：**9600, 8, N, 1**（9600 波特率 / 8 数据位 / 无校验 / 1 停止位）；同一总线上所有从站必须一致。

### 1.4 VLAN 与防火墙白名单规划

| VLAN | 子网 | 承载 | 网关地址示例 | 访问控制 |
|---|---|---|---|---|
| **VLAN10** | 192.168.10.0/24 | 电力仪表与 PLC 控制专网：ADL400 ×5、汇川 PLC、串口服务器现场口 | 192.168.10.1 | 仅允许 VLAN30 采集引擎主动发起 :502；**禁止** L4 直连 |
| **VLAN20** | 192.168.20.0/24 | 光伏与储能新能源专网：PV-INV-01（192.168.20.11）、BMS-01（192.168.20.21:502）、PCS-01（192.168.20.22:502） | 192.168.20.1 | 仅允许 VLAN30 采集/遥调；**禁止** L4 直连 :502 |
| **VLAN30** | 192.168.30.0/24 | SCADA/EMS 监控主站：Rust :8018、PostgreSQL :5436、值班 HMI | 192.168.30.1 | 可受控访问 VLAN10/20；北向经 DMZ 出 |

**工业防火墙白名单（唯一北向出口）**：

```
允许：L4(10.10.0.0/16) → DMZ → VLAN30:TCP/8018   （开放 API，X-API-Token 鉴权）
允许：VLAN30:8018 → L4 Webhook 接收端 http(s)    （告警事件出站，单向）
拒绝：L4 → VLAN10 任意端口
拒绝：L4 → VLAN20 任意端口 / TCP 502             （绝不把现场设备暴露给办公网）
拒绝：VLAN10 ↔ VLAN20 互访                       （控制网与新能源网横向隔离）
拒绝：外网 → :502 / :4840 / :5436                 （数据库与现场端口不出 DMZ）
```

> 讲解要点：**OT 侧按 Purdue 分层做广播域隔离，IT 侧只看见一个 HTTPS/REST 门面**。PLC/BMS/PCS 的 :502 永远不会被办公网扫到，数据库 :5436 也不出 DMZ；即使 L4 被攻破，攻击面止步于 8018 的令牌鉴权接口。这正是 IEC 62443 "分区 + 管道（Zone & Conduit）" 的落地。

### 1.5 北向集成（真实存在的接口，不编造）

| 对接对象 | 真实机制 | 载荷 |
|---|---|---|
| MES / CMMS | **出站 Webhook**（`fire_webhook`），事件 `alarm.triggered` / `alarm.closed` | `{"event":"alarm.triggered","alarm":{id,device_id,severity,message,status,...}}`，消费侧转成缺陷工单 |
| ERP 财务总账 | 定时任务拉取 **`GET /api/v1/open/export/day`**（`X-API-Token`），返回 `text/csv` | CSV 列：`device,total_kwh,peak_kwh,flat_kwh,valley_kwh,cost` |
| 第三方平台/PLC | **`POST /api/v1/open/ingest`**（南向推数） | `{"device":"ext-temp-01","metric":"temp","value":26.5}` |
| BI / 能管 | `GET /api/v1/open/latest` · `/history` · `/alarms` · `/devices` | JSON，令牌鉴权 |

`/api/v1/open/export/day` 的 CSV 由 `tou.tariff`（peak 1.0 / flat 0.62 / valley 0.32 CNY）逐桶算出 `cost = peak×1.0 + flat×0.62 + valley×0.32`，文件名 `energy-{day}.csv`——**这就是 ERP 电费入账与光储结算的原始凭证数据源**。

---

## 2. 全套 Modbus 寄存器点表

### 2.0 点表约定（先讲清楚列，面试官才知道你懂工程化）

| 列名 | 含义 |
|---|---|
| 设备角色 | 一次系统里的物理角色（`inbound` / `feeder` / `pv_ess` / `water`） |
| 从站或 Unit ID | RTU 叫 Slave Address（1~247），TCP 叫 MBAP 里的 Unit ID |
| 链路与协议 | RS485 Modbus-RTU / 工业以太网 Modbus-TCP :502 / OPC UA :4840 |
| 寄存器地址 Hex(Dec) | 协议地址（0x 基址，非 4xxxx 牌号），括号给十进制便于手算 |
| 功能码 | `03H` 读保持寄存器、`04H` 读输入寄存器、`06H` 写单寄存器、`10H` 写多寄存器 |
| 数据类型与字节序 | `UInt16` / `Int16` / `UInt32` / `Int32`，Big-Endian **ABCD**（详见 §3.6） |
| 缩放系数 | 工程值 = 原始值 × scale；电流/电压/功率/电能各不同，**不能一刀切** |
| 物理测点 | 工程量与单位 |

> 这些列在代码里就是 `metrics` 表的 `reg_addr / reg_len / scale / unit / fc / dtype / endian / rw`——**换一块表只 `UPDATE metrics`，不改一行 Rust**。面试时我会直接指这句 SQL：
> `UPDATE metrics SET reg_addr=0x2000, scale=0.1 WHERE device_id='meter-main' AND metric='u';`

### 2.1 安科瑞 ADL400 进线总表 / 分路表

链路：**RS485 手拉手菊花链 → USR-N540 串口服务器 → Modbus-TCP**；波特率 **9600,8,N,1**；`03H` 读保持寄存器；**UInt16 / Int32 Big-Endian ABCD**。

| 设备角色 | 从站或 Unit ID | 链路与协议 | 寄存器地址 Hex(Dec) | 功能码 | 数据类型与字节序 | 缩放系数 | 物理测点 |
|---|---|---|---|---|---|---|---|
| 进线总表 ADL400 `meter-main` | Slave **0x01** | RS485 Modbus-RTU → 串口服务器透传 | 0x2000 (8192) | 03H | UInt16, ABCD | ×0.1 | U 三相电压 V |
| 进线总表 ADL400 `meter-main` | Slave 0x01 | 同上 | 0x2002 (8194) | 03H | UInt16, ABCD | ×0.01 | I 电流 A |
| 进线总表 ADL400 `meter-main` | Slave 0x01 | 同上 | 0x2012 (8210) | 03H | Int32, ABCD | ×0.1 | P 有功功率 kW |
| 进线总表 ADL400 `meter-main` | Slave 0x01 | 同上 | 0x2015 (8213) | 03H | UInt16, ABCD | ×0.001 | PF 功率因数 |
| 进线总表 ADL400 `meter-main` | Slave 0x01 | 同上 | 0x2017 (8215) | 03H | UInt16, ABCD | ×0.01 | f 频率 Hz |
| 进线总表 ADL400 `meter-main` | Slave 0x01 | 同上 | **0x2100 (8448)** | 03H | Int32, ABCD（双字） | ×0.1 | EPi 正向有功电能 kWh |
| 1# 空压机回路 `feeder-ac` | Slave **0x02** | 同上 | 0x2000 / 0x2002 / 0x2012 / 0x2015 / 0x2100 | 03H | 同上 | 同上 | U/I/P/PF/EPi |
| 2# 注塑机回路 `feeder-im` | Slave **0x03** | 同上 | 0x2000 / 0x2002 / 0x2012 / 0x2015 / 0x2100 | 03H | 同上 | 同上 | U/I/P/PF/EPi（过载红线 62kW） |
| 3# 照明与空调 `feeder-lt` | Slave **0x04** | 同上 | 0x2000 / 0x2002 / 0x2012 / 0x2015 / 0x2100 | 03H | 同上 | 同上 | U/I/P/PF/EPi |
| 4# 机房与办公 IT `feeder-it` | Slave **0x05** | 同上 | 0x2000 / 0x2002 / 0x2012 / 0x2015 / 0x2100 | 03H | 同上 | 同上 | U/I/P/PF/EPi |

> 从站 **0x01~0x05** 与 VLAN10 的"进线→母排→4 分回路"一次接线图一一对应；`meter-main` 的 `rated_i=400A` 用于负载率口径。
> **⚠ 现场纪律**：不同批次/型号地址可能不同——上电前必须用厂商调试软件（安科瑞 AcrelBus / Modbus Poll）实读一遍核实，再 `UPDATE metrics`，绝不照抄样例点表。这句话面试官一听就知道我接过真表。

### 2.2 光伏逆变器 PV-INV-01（SG110CX）

链路：**RS485-Bus2（192.168.20.11 经 USR-N540 网关进 VLAN20）**；`04H` 读输入寄存器（逆变器厂家把实时量放在 3xxxx 输入寄存器区，与电表的 4xxxx 保持寄存器区分开）。

| 设备角色 | 从站或 Unit ID | 链路与协议 | 寄存器地址 Hex(Dec) | 功能码 | 数据类型与字节序 | 缩放系数 | 物理测点 |
|---|---|---|---|---|---|---|---|
| 1# 光伏并网逆变器 SG110CX | Slave **0x11 (17)** | RS485 Modbus-RTU（Bus2）→ 网关透传 | 0x3000 (12288) | **04H** | UInt16, ABCD | ×0.1 | 直流母线电压 V |
| 1# 光伏并网逆变器 SG110CX | Slave 0x11 | 同上 | 0x3004 (12292) | **04H** | **UInt32**, ABCD | ×0.1 | 并网有功功率 kW |
| 1# 光伏并网逆变器 SG110CX | Slave 0x11 | 同上 | 0x301A (12314) | **04H** | UInt16, ABCD | ×0.1 | IGBT 模块温度 ℃ |
| 1# 光伏并网逆变器 SG110CX | Slave 0x11 | 同上 | 0x3020 (12320) | **04H** | **UInt32**, ABCD | ×0.1 | 日/累计发电量 kWh |

> 真实抓帧（`comm_logs` 种子）：`11 04 30 00 00 01 3C 5A` → `11 04 02 1C 0A F0 34`，`0x1C0A=7178 ×0.1 ≈ 717.8V` 量级直流母线；备注写明"网关 RTU→TCP 透明转发"。IGBT 温度是巡检表 `XJ-202611-001` 的"48.2C"来源。

### 2.3 储能 BMS-01（电池管理系统）

链路：**工业以太网 Modbus-TCP :502**，Unit **0x01**，IP **192.168.20.21:502**，VLAN20；`03H` 读保持寄存器。

| 设备角色 | 从站或 Unit ID | 链路与协议 | 寄存器地址 Hex(Dec) | 功能码 | 数据类型与字节序 | 缩放系数 | 物理测点 |
|---|---|---|---|---|---|---|---|
| 1# 储能锂电簇 BMS | Unit **0x01 :502** | Modbus-TCP | 0x0100 (256) | 03H | UInt16, ABCD | ×0.1 | SOC 荷电状态 % |
| 1# 储能锂电簇 BMS | Unit 0x01 :502 | Modbus-TCP | 0x0101 (257) | 03H | UInt16, ABCD | ×0.1 | SOH 健康度 % |
| 1# 储能锂电簇 BMS | Unit 0x01 :502 | Modbus-TCP | 0x0108 (264) | 03H | UInt16, ABCD | ×0.001 | 单体最大压差 V |
| 1# 储能锂电簇 BMS | Unit 0x01 :502 | Modbus-TCP | 0x010A (266) | 03H | UInt16, ABCD | ×0.1 | 电芯温度 ℃ |

> 压差是储能安全的核心量：`inspections` 里"簇间压差 16mV (<25mV)"、`defects` 里 `QF-202611-002` 记 "32mV > 25mV 阈值 → 隔离该簇、均衡充放电"，都从这个点位来。SOC 78.5% 是巡检读数，也是峰谷套利策略的约束条件。

### 2.4 储能 PCS-01（双向变流器）与遥调

链路：**工业以太网 Modbus-TCP :502**，Unit **0x02**，IP **192.168.20.22:502**，VLAN20；`03H` 读、`06H` 写单寄存器。

| 设备角色 | 从站或 Unit ID | 链路与协议 | 寄存器地址 Hex(Dec) | 功能码 | 数据类型与字节序 | 缩放系数 | 物理测点 / 读写 |
|---|---|---|---|---|---|---|---|
| 1# 储能变流器 PCS-100kW | Unit **0x02 :502** | Modbus-TCP | 0x0200 (512) | 03H | **Int16**, ABCD | ×0.1 | 有功功率 kW（**正放负充**），R |
| 1# 储能变流器 PCS-100kW | Unit 0x02 :502 | Modbus-TCP | **0x0210 (528)** | **06H** | **Int16**, ABCD | ×0.1 | 功率设定 kW，**RW 遥调** |
| 1# 储能变流器 PCS-100kW | Unit 0x02 :502 | Modbus-TCP | 0x0202 (514) | 03H | UInt16, ABCD | ×0.1 | 交流电压 V，R |
| 1# 储能变流器 PCS-100kW | Unit 0x02 :502 | Modbus-TCP | 0x0206 (518) | 03H | UInt16, ABCD | ×0.01 | 电网频率 Hz，R |

**遥调（削峰填谷）两个必背例子**：

| 场景 | 时段电价 | 目标 | 写入寄存器 0x0210 的原始值 | 十六进制 | 工程值 | 业务含义 |
|---|---|---|---|---|---|---|
| **谷电充电** | valley 0.32 | −80.0 kW | **−800** | `0xFCE0`（Int16 补码） | −800 × 0.1 = **−80.0 kW 充电** | 低价买电存进电池 |
| **峰电放电** | peak 1.00 | +120.0 kW | **+1200** | `0x04B0` | 1200 × 0.1 = **+120.0 kW 放电** | 高价时段替代电网供电 |

> 讲解口径：`0x0200` 是**遥测**（只读，正=放电/负=充电），`0x0210` 是**遥调**（`06H` 可写）；写到 `metrics.rw='RW'` 列。下发前走"**选择—返校—执行**"：先读回当前设定值确认对象，写完后立刻再读 `0x0200` 校验实际出力方向，全过程写 `audit_logs`。这就是电力系统"遥控/遥调必须返校"的规程，不是普通 CRUD。

### 2.5 汇川 PLC Easy521 / 变频供水泵（恒压供水教学场景）

| 设备角色 | 从站或 Unit ID | 链路与协议 | 寄存器地址 Hex(Dec) | 功能码 | 数据类型与字节序 | 缩放系数 | 物理测点 |
|---|---|---|---|---|---|---|---|
| 汇川 PLC Easy521（恒压供水） | **Slave 10 (0x0A)** | RS485 Modbus-RTU / OPC UA :4840（二期，保持寄存器 4xxxx 区） | 厂商点表（压力 SP、频率给定、泵状态） | 03H / OPC UA Read | UInt16, ABCD | 按厂商手册 | 管网压力 MPa、变频器频率 Hz、流量 m³/h、泵功率 kW |

- 该场景在 `sim.rs` 里是完整闭环：**压力设定 SP → PI 调节 → 变频器调频 → 泵功率 ∝ f³ → 管网压力回读**，叠加用水高峰扰动与"压力低"告警（规则 `ws-pump.pressure < 0.38MPa` 持续 5s、滞回 0.02）。
- `metrics` 中 `ws-pump` 的 `pressure/f/flow/p` 四条 `reg_addr=NULL`（当前由仿真/OPC UA 桥供给），接真 PLC 时按汇川手册补 `reg_addr` 并把 `driver` 切 `rtu`——**与电表共用同一采集层，不改代码**。
- 恒压供水不计入配电能耗报表口径（`devices.kind='water'`），讲解时我会主动说明这个"报表口径隔离"的设计，避免面试官问"泵的电费算哪去了"。

### 2.6 工程值换算速查（面试白板题）

```
工程值 = 原始值 × scale          原始值 = 工程值 ÷ scale

U   : 381.4 V  ← 0x0EE6 = 3814 × 0.1
U   : 380.5 V  ← 3805 = 0x0EDD × 0.1      （演示台词里用的瞬时读数）
I   : 123.45 A ← 12345 × 0.01
P   : 62.0 kW  ← 620 × 0.1                 （注塑机过载红线）
PF  : 0.923    ← 923 × 0.001
EPi : 12345.6 kWh ← 123456 × 0.1（Int32 双字）
SOC : 78.5 %   ← 785 × 0.1
PCS : −80.0 kW ← −800 × 0.1（Int16 补码 0xFCE0）
f   : 50.00 Hz ← 5000 × 0.01
```

---

## 3. Modbus 报文帧字节级拆解 + CRC16 校验说明

### 3.1 主站请求 TX：`01 03 20 00 00 02 CF CB`

| 偏移 | 字节 | 字段 | 解释 |
|---|---|---|---|
| 0 | `01` | 从站地址 Slave Address | 点名 **0x01 进线总表 ADL400**；RS485 总线上其余从站静默 |
| 1 | `03` | 功能码 Function Code | **03H = Read Holding Registers**（读保持寄存器） |
| 2–3 | `20 00` | 起始地址 Start Address | 大端 `0x2000` = 8192，即 U 电压寄存器 |
| 4–5 | `00 02` | 寄存器数量 Quantity | `0x0002` = 读 **2 个寄存器**（4 字节，兼容 32 位量） |
| 6–7 | `CF CB` | **CRC16 校验** | 对前 6 字节计算，**低字节在前**（Little-Endian 附加） |

帧结构（Modbus-RTU ADU）：

```
┌────────┬────────┬──────────────┬──────────────┬─────────────┐
│ Slave  │  FC    │ Start Addr   │ Quantity     │ CRC16       │
│ 1 byte │ 1 byte │ 2 bytes (BE) │ 2 bytes (BE) │ 2 bytes(LE) │
│  01    │  03    │  20 00       │  00 02       │  CF CB      │
└────────┴────────┴──────────────┴──────────────┴─────────────┘
  RTU 帧靠 3.5 个字符的静默间隔定界（无起止符），故 CRC 必须逐字节正确
```

### 3.2 从站响应 RX：`01 03 04 00 00 0E E6 7F D9`（18 ms）

| 偏移 | 字节 | 字段 | 解释 |
|---|---|---|---|
| 0 | `01` | 从站地址 | 回显 0x01，主站确认"是对的人回的" |
| 1 | `03` | 功能码 | 回显 03H；若异常则返回 `83H`（FC\|0x80）+ 异常码 |
| 2 | `04` | 字节数 Byte Count | 后续数据 **4 字节**（2 寄存器 × 2 字节） |
| 3–6 | `00 00 0E E6` | 寄存器数据 | Big-Endian ABCD 的 32 位字 = `0x00000EE6` = **3814** |
| 7–8 | `7F D9` | CRC16 | 对前 7 字节计算，低字节在前 |
| — | 18 ms | 响应时间 | `comm_logs.latency_ms`；RTU 9600bps 下这帧约 9 字节 ≈ 9×10bit/9600 ≈ 9.4ms，加上从站处理与总线周转，18ms 属正常量级 |

**工程量换算**：

```
原始值 0x00000EE6 = 3814
U = 3814 × 0.1 = 381.4 V        ← 三相进线电压，落在 360~400V 正常带内
（对照告警规则：meter-main.u > 400V 过压 / < 360V 欠压，持续 10s、滞回 2V）
```

> `comm_logs` 里这行的 `handling_note` 原文就是："03H 读保持寄存器；原始 0x0EE6=3814 ×0.1=381.4V，CRC16 校验通过"。**采集引擎把 TX/RX 十六进制、耗时、状态、处置备注全部落库**，排障时不用再扛笔记本去现场抓包。

### 3.3 Modbus-TCP 的 MBAP 头（7 字节）与 RTU 的差别

以"谷段给 PCS-01 下发 `p_set = −800` → −80.0kW 充电"为例，构造一帧 `06H` 写单寄存器：

```
00 01  00 00  00 06  02   06  02 10  FC E0
└─┬─┘  └─┬─┘  └─┬─┘  │    │   └─┬─┘  └─┬─┘
  │      │      │     │    │     │      └─ 写入值 0xFCE0 = −800 (Int16 补码) → −80.0 kW
  │      │      │     │    │     └──────── 寄存器地址 0x0210 (p_set)
  │      │      │     │    └────────────── 功能码 06H 写单寄存器
  │      │      │     └─────────────────── Unit ID = 0x02 (PCS-01)
  │      │      └───────────────────────── Length = 0x0006 (Unit + PDU 共 6 字节)
  │      └──────────────────────────────── Protocol ID = 0x0000 (Modbus)
  └─────────────────────────────────────── Transaction ID = 0x0001 (请求/响应配对)
```

| MBAP 字段 | 长度 | 值 | 作用 |
|---|---|---|---|
| Transaction ID | 2 B | `00 01` | 事务标识，主站自增；并发多请求时把响应对回原请求 |
| Protocol ID | 2 B | `00 00` | 协议标识，0 = Modbus/TCP |
| Length | 2 B | `00 06` | **其后**字节数（Unit ID + PDU），不含 MBAP 自身 6 字节 |
| Unit ID | 1 B | `02` | 单元标识，等价于 RTU 的从站地址，用于网关后挂多台串口设备 |

**RTU 与 TCP 的关键差别（面试高频）**：

| 对比项 | Modbus-RTU | Modbus-TCP |
|---|---|---|
| 帧定界 | **3.5 字符静默间隔** | TCP 流 + MBAP `Length` 字段定长 |
| 地址 | ADU 首字节 Slave Address | MBAP 的 Unit ID |
| 差错校验 | **CRC16（2 字节，低字节在前）** | **无 CRC**——TCP/IP 已有链路层/传输层校验，靠 MBAP 头 + TCP 保证 |
| 附加头 | 无 | **MBAP 7 字节**（事务/协议/长度/单元） |
| PDU | 完全相同（FC + 数据） | 完全相同 |
| 物理层 | RS485 半双工，主从轮询，9600,8,N,1 | 以太网全双工，:502，可并发多事务 |

> 一句话背熟：**"PDU 不变，ADU 换壳"**——RTU 把 PDU 套上 `Slave + CRC16`，TCP 把 PDU 套上 `MBAP 头`，功能码/寄存器地址/数据的语义完全一致。所以 USR-N540 串口服务器能做 RTU→TCP **透明转换**：它剥掉 CRC、补上 MBAP、把 Slave 搬进 Unit ID，反向亦然。

### 3.4 CRC16 计算要点

- **多项式**：`0xA001`（即 `x^16 + x^15 + x^2 + 1` 的反转形式，CRC-16/MODBUS）
- **初值**：`0xFFFF`
- **输入**：从 Slave Address 到数据区最后一个字节（**不含 CRC 字段本身**）
- **输出附加顺序**：**低字节在前**（先发 `crc & 0xFF`，再发 `crc >> 8`）
- **异或输出**：不再做最终 XOR（区别于 CRC-16/CCITT-FALSE）
- **校验动作**：接收方对"含 CRC 在内的整帧"重算，结果应为 `0x0000`

```python
def crc16_modbus(data: bytes) -> int:
    crc = 0xFFFF
    for b in data:
        crc ^= b
        for _ in range(8):
            if crc & 1:
                crc = (crc >> 1) ^ 0xA001   # 反向多项式
            else:
                crc >>= 1
    return crc & 0xFFFF

# 低字节在前附加：
# crc = crc16_modbus(bytes.fromhex("01 03 20 00 00 02"))
# frame = payload + bytes([crc & 0xFF, crc >> 8])
```

### 3.5 ⚠ 抓帧核对纪律（面试加分点：CRC 必须自己复算）

> 本文所有帧尾的 2 字节 CRC **均由 0xA001 / 0xFFFF 实算得出，与系统 `src/web.rs::crc16()` 输出逐字节一致**，可直接拿去和抓帧工具比对。
>
> 用 `01 03 20 00 00 02` 按 §3.4 复算得到 `0xCBCF`，低字节在前附加 → `CF CB`；接收帧 `01 03 04 00 00 0E E6` 复算得到 `0xD97F` → `7F D9`。
>
> 现场纪律：**抓帧工具（Modbus Poll / Wireshark / 串口助手）显示的尾字节只要与复算值不一致，就说明链路被改动、波特率/字节序不对，或抓的根本是另一帧**——不要相信眼睛，要相信算法。
>
> 这条纪律对应的真实故障就是 §4 的"屏蔽层两端接地 → 偶发 CRC 误码"和 `defects.QF-202611-004`。**会复算 CRC 的人，和只会贴报文的人，在现场是两种工种。**

### 3.6 大端字节序 ABCD / CDAB / BADC / DCBA 与排查

32 位量由 **2 个 16 位寄存器**拼成，把四个字节记作 `A B C D`（A 为最高有效字节）：

| 记法 | 字节流 | 含义 | 常见于 |
|---|---|---|---|
| **ABCD** | `A B C D` | 标准 Modbus 大端：寄存器顺序正常、寄存器内字节正常 | 本项目 `metrics.endian` 默认值（ADL400/BMS/PCS/逆变器全部 ABCD） |
| **CDAB** | `C D A B` | **字交换**（Word Swap）：两寄存器对调，寄存器内字节不变 | 部分国产电表/逆变器 |
| **BADC** | `B A D C` | **字节交换**（Byte Swap）：每个寄存器内部两字节对调，字序不变 | 某些 PLC / 网关配置错误 |
| **DCBA** | `D C B A` | **全反转**（小端）：字序与字节序同时反 | x86 内存序被直接塞进报文 |

以 `0x12345678` 为例：ABCD=`12 34 56 78`，CDAB=`56 78 12 34`，BADC=`34 12 78 56`，DCBA=`78 56 34 12`。

**排查方法（现场按顺序做）**：

1. **用已知工程量倒推**：拿万用表/钳形表读一个稳定量（如进线 381.4V），看原始 4 字节；若出现 `0x0EE60000` 之类的"天文数字"，基本是字序或字节序错。
2. **抓帧对齐**：从 `comm_logs.tx_hex/rx_hex` 取原始字节，别看已经乘过 scale 的画面值——画面值会掩盖字节序错误。
3. **四种组合逐一试算**：把 4 字节按 ABCD/CDAB/BADC/DCBA 重排，选出乘 scale 后落在合理量程的那一个。
4. **配置化修正**：在 `metrics.endian` 改一个字段即可，不碰采集代码；Modbus Poll 里对应 "Swapped / Little-endian" 显示选项。
5. **区分 16 位与 32 位**：`reg_len=2` 的 UInt16/Int16 不存在字序问题，只有 `reg_len=4` 的 UInt32/Int32（EPi 电能、并网有功、发电量）才需要试四种组合。
6. **Int16 补码别当无符号**：PCS `0xFCE0` 若被当 UInt16 读成 64512 × 0.1 = 6451.2kW，明显荒谬；看到"大得离谱的正数"先怀疑符号与字节序。

---

## 4. 五类常见现场断线故障排错实战手册

### 4.1 故障排查表（现象 / 可能原因 / 排查动作 / 验证）

| # | 现象 | 可能原因 | 排查动作 | 验证 |
|---|---|---|---|---|
| 1 | **全站 RTU 不通，所有从站零响应** | **RS485 A+ / B− 极性接反**（最常见，新手第一错） | 断电后核对 A+→A+、B−→B−；万用表测 A/B 间差分电压（空闲应有几百 mV 偏置）；整段总线统一标记线号/色标 | 单独挂 1 台从站用 Modbus Poll 读 `0x2000`，能回 `01 03 04 ...` 即极性正确 |
| 2 | **长线偶发误码、时通时断、CRC 错** | **120Ω 终端电阻缺失或多余（>2 个）**；或**屏蔽层两端接地**形成地环流 | 只在总线**最远两端**各留 1 个 120Ω，中间表箱一律拆除；屏蔽层改**控制室单端接地**；示波器看 A/B 波形有无过冲/振铃 | 连续轮询 30 分钟无 `timeout`、无 CRC 重试；`comm_logs.status` 全 `ok` |
| 3 | **网关在网但主站读不到，TCP 连不上** | **串口网关 IP / 端口不通与防火墙**：USR-N540 IP 与 VLAN 规划不符、:502 被交换机 ACL/工业防火墙拦截、网关串口参数未设成 9600,8,N,1、RTU→TCP 映射的 Unit ID/Slave 绑定错 | `ping 网关 IP` → `telnet 网关 502` → 核对网关 Web 配置页（波特率/校验/透传模式/从站映射表）→ 查三层交换机 ACL 与 DMZ 白名单是否只放行 8018 | `nc -vz <gw> 502` 通；主站 TX 发出后 18~30ms 内收到合法 RX |
| 4 | **个别从站时通时断 / 只有某台不回** | **从站地址冲突或波特率不匹配**（9600,8,N,1 不一致；两台都设成 0x01） | 逐台单独上电、用厂商软件改唯一 Slave 地址（0x01~0x05 / 0x11 / 10）；全总线统一 9600,8,N,1；核对 `devices.meta.slave` 与 `metrics` 的绑定 | 主站按点名表逐个单播，每台都能单独回帧；总线合上后轮询表全绿 |
| 5 | **画面数值冻结、通信灯变灰** | **通信看门狗触发后的断链闭环**（非"原因"而是"结果"，但要能定位到上面 4 类根因） | 按 §4.2 状态机查 `comm_logs` 的 `fault_stage`（Retry 1/3 → 3/3 → Auto-Heal）与 `handling_note`；定位到具体表箱端子/终端电阻/屏蔽层 | 收到合法 RX 帧后 `alarm.comm` 自动 `closed`，画面由灰 `OFFLINE` 恢复为实时值 |

> 现场口诀（背熟，来自 `docs/硬件接入`）：**A/B 接反→全站不通；共地未接→数据丢包；终端电阻缺失→长线误码；从站地址冲突→时通时断；波特率不齐→CRC 错。**
> 安全前置：RS485 二次侧是弱电，接线不断电也安全；但凡动**电压/电流回路**必须先停电→验电→LOTO 挂牌（见 §5.4）。

### 4.2 断线闭环状态机（连续 3 次重试 → 15s 看门狗 → 冻结置灰 → 自愈消警）

```mermaid
flowchart LR
  A["正常轮询<br/>2s 一拍 · RX 18ms · status=ok"] -->|"Rx Timeout > 500ms"| B["Retry 1/3<br/>comm_logs.fault_stage"]
  B -->|仍超时| C["Retry 2/3"]
  C -->|仍超时| D["Retry 3/3<br/>连续 3 次超时"]
  D --> E["15s 通信看门狗触发<br/>alarm.comm · severity=CRITICAL<br/>Webhook alarm.triggered"]
  E --> F["冻结最后有效值<br/>画面置灰 OFFLINE<br/>生成排障记录 comm_logs"]
  F -->|"现场排查：端子紧固 / 补 120Ω / 单端接地"| G["收到合法 RX 帧<br/>CRC 校验通过"]
  G --> H["auto-heal 自动消警<br/>alarm.closed · Webhook alarm.closed<br/>恢复实时值"]

  style E fill:#ffe0e0,stroke:#c00
  style F fill:#eeeeee,stroke:#888
  style H fill:#e0ffe0,stroke:#080
```

**与代码的对应（指给面试官看）**：

- `collector.rs`：通信故障时**整台冻结最后有效值**——`if s.comm { 更新 ts/values } else { entry.comm=false }`，不写脏数据、不把 0 当真实读数入库。
- `alarm.rs`：`fresh = now*1000 − dev.ts < 15_000`；`!fresh && !dev.comm` 且未在 `comm_fired` 中 → `create_alarm(... "comm","critical","alarm.comm")`。恢复条件命中即 `close_alarm`，**自愈自动消警**，无需人工点关闭。
- `alarm_rules` 的**阈值 + duration_s + hysteresis** 三件套：越限要**持续**够 `duration_s` 才触发；回落到 `threshold ± hysteresis` 才清除——这就是 SCADA 报警去抖，避免临界抖动刷屏（例：过压 400V 持续 10s、滞回 2V，即回到 <398V 才消警）。

### 4.3 `comm_logs` 真实种子记录回放（演示时直接翻这一页）

| 时间 | 设备 | 协议 | TX | RX | 耗时 | status | fault_stage | 处置备注 |
|---|---|---|---|---|---|---|---|---|
| −9 min | meter-main | Modbus-RTU | `01 03 20 00 00 02 CF CB` | `01 03 04 00 00 0E E6 7F D9` | 18 ms | ok | 轮询正常 | 原始 0x0EE6=3814 ×0.1=381.4V，CRC16 校验通过 |
| −8 min | pv-inv-01 | Modbus-RTU | `11 04 30 00 00 01 3C 5A` | `11 04 02 1C 0A F0 34` | 22 ms | ok | 轮询正常 | 04H 读输入寄存器；直流母线，网关 RTU→TCP 透明转发 |
| −7 min | ess-pcs-01 | Modbus-TCP | `00 01 00 00 00 06 02 06 02 10 ...` | 同帧回显 | 12 ms | ok | 遥调成功 | 谷段写 `p_set=−800 → −80.0kW`；MBAP 头无 CRC，06H 写单寄存器 |
| −6 min | meter-main | Modbus-RTU | `01 03 20 00 00 02 CF CB` | **(无响应)** | **520 ms** | timeout | **Retry 1/3** | Rx Timeout >500ms，检查 RS485 A+/B− 极性 |
| −5 min | meter-main | Modbus-RTU | `01 03 20 00 00 02 CF CB` | **(无响应)** | **520 ms** | timeout | **Retry 3/3** | 连续 3 次超时；15s 看门狗触发 `alarm.comm(CRITICAL)`，冻结最后有效值并置灰 OFFLINE |
| −4 min | meter-main | Modbus-RTU | `01 03 20 00 00 02 CF CB` | `01 03 04 00 00 0E E2 7E 1A` | 19 ms | ok | **Auto-Heal 自动恢复** | 现场排查：3# 表箱 RS485 B− 端子松动，紧固并补接 120Ω 终端电阻；收到合法 RX 帧自动消警 |

> 这一组记录把 §3 的报文、§4.1 的根因、§4.2 的状态机**串成一条可回放的时间线**。面试时我会说："我不是只画了个告警框图，我把每一帧 TX/RX、每一次重试、每一条处置备注都落进 `comm_logs`，故障复盘是可以审计的。"

---

## 5. 光伏/储能运维四大单据与 ERP/CMMS 业务字段映射

### 5.1 全链路业务流

```mermaid
flowchart TB
  I["① 巡检表 Inspection Checklist<br/>inspections · 白/夜班 · 区域 · 逐项打勾"]
  D["② 缺陷台账 Defect Ledger<br/>defects · 危急 24h / 严重 72h / 一般 月度"]
  S["③ 电气倒闸操作票 Switching Ticket<br/>switching_tickets · 唱票→监护复诵→执行→五防校核→LOTO"]
  W["④ CMMS 维护工单 & ERP 成本归集<br/>work_orders / wo_notes / erp_notif_no"]
  G["日报月报 → ERP 总账<br/>/api/v1/open/export/day · 峰平谷电费 + 光储结算"]

  I -->|"abnormal_count > 0<br/>异常项转缺陷"| D
  D -->|"需停电/隔离处置<br/>开票"| S
  S -->|"检修完成 · 回填处理记录"| W
  D -->|"生成 ERP PM 缺陷通知单 erp_notif_no<br/>关联 wo_id"| W
  W -->|"工单关单 · 成本归集"| G
  I -->|"巡检结果同步 erp_sync_status"| G
  D -->|"Webhook alarm.triggered 也能直接转缺陷"| W

  style D fill:#fff3cd,stroke:#b8860b
  style S fill:#ffe0e0,stroke:#c00
  style G fill:#e0e8ff,stroke:#36c
```

**单据驱动关系**：巡检发现异常 → 登记缺陷并按等级定时限 → 需要停电/隔离的缺陷开倒闸操作票 → 操作票执行完生成 CMMS 工单并回填处理记录 → 工单关单归集成本 → 日报月报（峰平谷电费、光伏自发自用、储能套利）定时进 ERP 总账。告警 Webhook 也可以跳过巡检直接生成缺陷工单，形成"**监控发现 → 运维处置 → 财务入账**"的完整闭环。

### 5.2 ① 巡检表 `inspections` 核心字段

| 字段 | 类型/示例 | 说明 |
|---|---|---|
| `code` | `XJ-202611-001` | 巡检单号，按年月流水 |
| `area` | 光伏阵列与逆变器区 / 储能集装箱 BMS/PCS / 0.4kV 低压配电室 | 巡检区域，对应 VLAN20 与 VLAN10 物理分区 |
| `inspector` | 王工 / 李工 / 张工 | 巡检人 |
| `shift` | 白班 / 夜班 | 班次 |
| `check_date` | CURRENT_DATE | 巡检日期 |
| `items` | JSONB 数组 `[{k, v, ok}]` | 逐项检查项与读数：组件热斑、MC4 接头、直流浪涌保护器窗口、逆变器风扇防尘网、IGBT 温度 48.2C、并网有功 62.3kW；储能侧簇间压差 16mV(<25mV)、液冷管路无凝露、七氟丙烷压力绿区、PCS 绝缘阻抗 2.4MΩ、SOC 78.5%、消防告警回路自检 |
| `abnormal_count` | 0 / 1 | 异常项计数，**>0 即触发转缺陷** |
| `erp_sync_status` | `synced` | 同步 ERP/CMMS 状态 |

> 注意 `XJ-202611-002` 的"消防告警回路自检 `ok:false`"→ `abnormal_count=1`，这就是从巡检表流向缺陷台账的触发点。安全无小事，消防回路自检不通过必须开单。

### 5.3 ② 缺陷台账 `defects` 核心字段与三级时限

| 字段 | 类型/示例 | 说明 |
|---|---|---|
| `code` | `QF-202611-001` | 缺陷单号 |
| `device_id` | `pv-inv-01` / `ess-bms-01` / `feeder-im` / `meter-main` | 关联 `devices.id`，与点表一一对应 |
| `level` | `critical` / `major` / `minor` | 缺陷等级（见下表时限） |
| `category` | 接头过热 / 电芯一致性 / 过负荷 / 通信质量 | 缺陷分类 |
| `title` | 1# 光伏逆变器 PV3 支路 MC4 端子发热 78℃ | 缺陷标题 |
| `phenomenon` | 红外测温 78℃，高于同工况支路 30℃ | 现象/实测证据 |
| `root_cause` | MC4 插头压接不实、接触电阻增大 | 根因分析 |
| `action_taken` | 停电重新压接并更换 MC4 端子，复测 41℃ | 处置措施与复测值 |
| `reporter` / `handler` | 王工 / 李工 | 报告人 / 处理人 |
| `wo_id` | 关联 `work_orders.id` | 转成的 CMMS 工单 |
| `erp_notif_no` | `ERP-PM-20261115-01` | ERP PM 缺陷通知单号 |
| `status` | `open` / `processing` / `closed` | 状态机 |
| `found_at` / `resolved_at` | 时间戳 | 发现/消缺时间，用于时限考核 |

| 等级 | 处置时限 | 本项目实例 |
|---|---|---|
| **危急 critical** | **24h** 内 | `QF-202611-002` 储能 BMS #3 簇第 14 节电芯压差 32mV > 25mV → 隔离该簇、均衡充放电、计划更换电芯 |
| **严重 major** | **72h** 内 | `QF-202611-001` 逆变器 MC4 端子发热 78℃；`QF-202611-003` 注塑机回路 62kW 触及过载红线 |
| **一般 minor** | **月度** 计划消缺 | `QF-202611-004` 进线总表 RS485 屏蔽层单端接地整改（偶发 CRC 误码） |

### 5.4 ③ 电气倒闸操作票 `switching_tickets`：唱票 / 监护 / 五防 / LOTO

| 字段 | 类型/示例 | 说明 |
|---|---|---|
| `ticket_no` | `DZ-202611-001` | 操作票编号 |
| `title` | 2# 注塑机回路（QF2）由运行转冷备用及挂牌检修操作票 | 操作任务 |
| `task_mode` | 停电检修 / 孤岛检修 | 任务模式 |
| `target_device` | `feeder-im` / `ess-pcs-01` | 目标设备 |
| `operator` | 王工 | **操作人** |
| `guardian` | 李工 | **监护人**（不得由操作人兼任） |
| `duty_leader` | 张值长 | 值班负责人/签发人 |
| `steps` | JSONB `[{n, txt, done}]` | 逐项操作步骤与打勾状态 |
| `safety_checks` | JSONB `[{k, ok}]` | **五防校核**逐项确认 |
| `status` | `in_progress` / `completed` | 票面状态 |
| `issued_at` / `finished_at` | 时间戳 | 开票/终了时间 |

**标准流程（唱票 → 监护复诵 → 执行 → 五防校核 → LOTO）**：

> 监护人按票**唱票** → 操作人**手指设备双重编号复诵** → 监护人确认"对，执行" → 操作人动手 → 每执行一项在 `steps[].done` 打勾 → 全部完成后逐项核 `safety_checks` → 终了回填 `finished_at`。

`DZ-202611-001` 的真实 7 步（对应 `steps` JSONB）：

1. 核对模拟图与设备名称编号，确认 QF2 双重编号无误
2. 下发遥控分闸指令断开 QF2 断路器
3. 检查 QF2 三相电流归零、刀闸机械指示在"分"位
4. 拉开 QF2 负荷侧隔离开关
5. 在 QF2 负荷侧**验电确无电压**
6. 装设 #02 号**临时接地线**
7. 悬挂"**禁止合闸，有人工作！**"标示牌 **LOTO 挂牌上锁**并锁定

`DZ-202611-002`（储能 PCS 并网转孤岛检修）还额外要求"**直流侧验电并充分放电**"——电容性设备不放电不能挂接地线，这是光储现场区别于普通低压配电的关键一步。

**五防（防止误操作，逐项写进 `safety_checks`）**：

| # | 五防内容 | 现场含义 |
|---|---|---|
| 1 | 防止误分误合断路器 | 核对双重编号，只对票面设备操作 |
| 2 | 防止带负荷分合隔离开关 | 先断断路器、确认电流归零，再拉刀闸 |
| 3 | 防止带电挂接地线 | 先验电确无电压，再装接地线 |
| 4 | 防止带接地线合断路器 | 送电前先拆接地线、核对接地开关位置 |
| 5 | 防止误入带电间隔 | 遮栏/闭锁/标示牌，人员不得越过 |

**停电检修安全顺序（背熟）**：**停电 → 验电 → 挂牌（LOTO 上锁）→ 放电（电容性设备）→ 临时接地**。补充：CT 二次回路**严禁开路**、PT 二次回路**严禁短路**；E-stop 用常闭串联 + 安全继电器双通道，**不能只靠软件**；演示回路必须带 30mA 漏保。

### 5.5 ④ CMMS 工单 & ERP 成本归集字段映射表

| 业务单据字段 | 本系统表/字段 | CMMS / ERP 对应对象 | 说明 |
|---|---|---|---|
| 巡检异常 | `inspections.abnormal_count` / `items[].ok` | CMMS 巡检记录 / ERP PM 通知单来源 | `abnormal_count>0` 触发 |
| 缺陷单 | `defects.code / level / category / title / phenomenon / root_cause / action_taken` | ERP PM **缺陷通知单** `erp_notif_no` | 危急 24h / 严重 72h / 一般月度 |
| 缺陷转工单 | `defects.wo_id` → `work_orders.id` | CMMS **维护工单** | 告警也可一键转工单 |
| 工单处理留痕 | `wo_notes(wo_id, note, author, created_at)` | CMMS 工单活动记录 | 处理记录不可篡改，闭环时间归档 |
| 工单状态 | `work_orders.status` `open→processing→closed` | CMMS 工单状态机 | `closed_at` 归档，关单回写 ERP 维护模块 |
| 告警联动 | `alarms` + Webhook `alarm.triggered/closed` | 事件驱动集成（钉钉/企微/ERP 工作流） | 实时推送，令牌/URL 白名单 |
| 操作审计 | `audit_logs(action, device, detail, result)` | 遥控/遥调/演练留痕 | `breaker_open/close`、`sim_overload` 等 |
| 能耗分桶 | `readings` + `tou.rate_of(h)` | ERP 成本核算口径 | peak/flat/valley 分桶 kWh |
| 结算单 | `GET /api/v1/open/export/day` CSV | **ERP 财务总账**入账凭证 | `device,total_kwh,peak_kwh,flat_kwh,valley_kwh,cost` |
| 成本中心 | `devices.id` / `kind`（`feeder` 计入、`water`/`pv_ess` 隔离） | ERP 成本中心 / 内部订单 | 报表口径隔离，避免重复计费 |

> 状态统一：告警 `active→acked→closed`，工单 `open→processing→closed`，缺陷 `open→processing→closed`，操作票 `in_progress→completed`——**四套状态机各自独立、用外键/单号互相引用**，这正是 ISA-95 L2 事件 → L3 事件管理 → L4 成本归集的分层落地（见 `docs/ISA-95映射与L3L4接口.md`）。

### 5.6 ERP 凭证示例与峰平谷 / 光储收益核算

**分时电价配置（真实 `settings.tou`，CNY/kWh）**：

```
peak   峰 1.00  → 时段 [8,11) ∪ [17,21)
flat   平 0.62  → 时段 [11,13) ∪ [21,23)
valley 谷 0.32  → 时段 [23,8)
cost = peak_kwh × 1.00 + flat_kwh × 0.62 + valley_kwh × 0.32
```

**ERP 记账分录（电费月账）**：

```
借：制造费用 — 电费（按成本中心/车间分摊）        ¥ (峰+平+谷 cost 合计)
    制造费用 — 电费（光储自发自用抵减，见下）      ¥ (红字冲减)
    贷：应付账款 — 供电局                          ¥ (向电网实付净额)
```

**三项收益核算口径（面试要能讲清差别）**：

| 项目 | 计算口径 | 入账方式 |
|---|---|---|
| **峰平谷分时电价** | 按 `tou.rate_of(h)` 把 `readings` 的 kWh 分桶，逐桶乘费率求和 | 电费成本，借"制造费用-电费" |
| **光伏自发自用折算节电收益** | 光伏并网有功（0x3004）× 同时段**电网电价** = 少向电网买的电 | 冲减电费成本（红字），不确认收入 |
| **储能峰谷套利净收益** | 谷段充电电量×0.32 为成本，峰段放电电量×1.00 为替代收益，净额 = 放电收益 − 充电成本 − 损耗 | 冲减电费成本 / 内部订单核算；PCS 遥调 `p_set` 的 ±80/±120kW 就是执行侧 |

> 讲解口径：我**不把光伏自用和储能套利当"营业收入"**，它们是**电费成本的抵减**，走成本中心而非收入科目——这一点能区分"懂业务"和"只会算数字"。`pv_ess` 设备不计入 feeder 电费口径（`kind='pv_ess'`），避免把自发自用又算一遍电费，报表口径隔离是刻意设计。

---

## 6. 五分钟面试现场演示台词剧本

> 用法：三段各约 1 分钟，第一人称口语化，可直接照念。全程开着 `http://127.0.0.1:8018` 看板，鼠标跟着台词走。**括号里是动作，不是台词**。

### 第 1 分钟【总览】——证明我懂一次系统与告警学

> （点开总览 SVG 一次接线图，鼠标停在 QF0 总进线断路器符号上）
>
> "先给您看这张一次接线图：进线 QF0 → 总表 → 母排 → 四个分回路，空压机、注塑机、照明空调、机房 IT，实时跑着 U/I/P/PF/EPi 和通信状态灯。我现在点一下 **QF0 总进线分闸**——您看，母线电压直接归零，下面四个回路哪怕开关还合着也没电，因为仿真里 `main_open` 一置位，全厂功率、电流、频率全清零，这就是真实的'总进线失电'逻辑，不是把某个数字改成 0。
>
> （点开审计页）"而且这一下分闸不是白点的：`audit_logs` 里记了 `breaker_open`、目标设备 `meter-main`、操作人和理由——**遥控必须留痕**，这是电力规程。
>
> （合闸，点「模拟注塑机过载」）"合回来，我给 2# 注塑机回路注入一个过载，+40kW、持续 90 秒。曲线抬头了，我点开趋势图，用十字准星能读到任意时刻的工程值。注意告警不是一超阈值就弹：我的规则是**阈值 62kW + 持续 15s + 滞回 3kW**——得连续超 15 秒才算数，回落到 59kW 以下才消警。这三件套就是 SCADA 报警去抖，现场最怕的就是临界抖动把值班手机刷爆。告警出来同时走 Webhook，配了就能推到手机。"

### 第 2 分钟【Modbus 与拓扑】——证明我懂 OT 现场与报文

> （切到实时点表 / 通信记录页，手指屏幕从左到右）
>
> "这一分钟我从最底下往上讲。最底下是 RS485 手拉手菊花链：RVSP 2×0.75 屏蔽双绞线，A+/B− 两根，两端各一个 120Ω 终端电阻，屏蔽层控制室单端接地，规约 **Modbus-RTU 9600,8,N,1**。安科瑞 ADL400 电表是 0x01 到 0x05，光伏逆变器 SG110CX 是 0x11 挂在 Bus2，储能 BMS/PCS 走工业以太网 **Modbus-TCP :502**，PLC 走 OPC UA :4840。中间 USR-N540 串口服务器做 RTU→TCP 透明转换，断网还能本地缓存。
>
> （指 VLAN 图）"往上三层交换机划了三个 VLAN：**VLAN10 192.168.10.0/24** 电力仪表和 PLC 控制专网，**VLAN20 192.168.20.0/24** 光伏储能 BMS/PCS 新能源专网，**VLAN30 192.168.30.0/24** 是我们监控主站，Rust 服务 :8018 加 PostgreSQL :5436。工业防火墙白名单只放行一个口——**TCP 8018 的北向开放 API**，办公网 10.10.0.0/16 绝对直连不到 VLAN10/20，更摸不到 :502。IT/OT 边界就卡在这儿。
>
> （指实时点表）"看这个数：**380.5V**。它背后是寄存器 **0x2000**、缩放 **0.1**、原始值 **3805 也就是 0x0EDD**——地址、缩放、功能码、字节序全在 `metrics` 表里，换块表一条 UPDATE 就完事，不改 Rust 代码。
>
> （切到 `comm_logs`）"右边这一栏是真实抓帧：`01 03 04 ...` 响应帧，还有一次断线的完整记录——**Rx Timeout 超过 500ms → Retry 1/3、2/3、3/3 → 15 秒通信看门狗触发 `alarm.comm` CRITICAL → 冻结最后有效值、画面置灰 OFFLINE**。现场查出来是 3# 表箱 B− 端子松动，紧固、补 120Ω 之后，收到合法 RX 帧就 **auto-heal 自动消警**。我不是画了个告警框图，每一帧 TX/RX、每一次重试、每一条处置备注都落库了，复盘能审计。"

### 第 3 分钟【光储运维 CMMS】——证明我懂电力规程与业务闭环

> （切到巡检/缺陷/操作票页）
>
> "最后一分钟讲光储运维怎么跟 ERP/CMMS 闭环，这块是我跟纯 SCADA 工程师最大的区别。先是**巡检表**：白班夜班、光伏区/储能集装箱/低压配电室，逐项打勾——组件热斑、MC4 接头、IGBT 温度、簇间压差、七氟丙烷压力、消防回路自检。哪天像 `XJ-002` 消防自检 `ok:false`，异常数变 1，就转**缺陷台账**：分危急 **24 小时**、严重 **72 小时**、一般月度三级，每条都带现象、根因、处置措施和复测值，同时生成 ERP PM 缺陷通知单号。
>
> （点开 `DZ-202611-001`）"需要停电处理的缺陷，就得开**倒闸操作票**。您看这七步，现场是**唱票 → 监护复诵 → 执行 → 逐项打勾**：核对双重编号、遥控分闸、验电流归零、拉隔离开关、**验电**、装临时接地线、最后挂'禁止合闸，有人工作'的 **LOTO 挂牌上锁**。旁边这五条是**五防**校核，一条都不能少——防误分误合、防带负荷拉刀闸、防带电挂接地线、防带接地线合闸、防误入带电间隔。储能那张票还多一条'直流侧验电并充分放电'，电容不放电不能挂地线。
>
> （切到报表）"操作票执行完回填处理记录、关 CMMS 工单，成本归集到 ERP。最后每天定时拉 `/api/v1/open/export/day` 出 CSV：**峰平谷分时电费**、光伏自发自用折算的节电收益、储能谷充峰放的套利净额，进 ERP 总账——借方'制造费用-电费'，贷方'应付账款-供电局'，光储那部分是**冲减成本**不是确认收入。所以从 RS485 的 A+/B− 两根线，到 ERP 的一张记账凭证，整条链我是自己一行行打通的。"

**收尾一句（可选）**："这套东西现在是仿真驱动在跑，`--features rtu` 的串口驱动接口已经预留好，真表到货当天把 `devices.driver` 切过去就行，采集、告警、工单、报表一条都不用改——**系统先行、硬件后接**，这是我做工业项目一贯的顺序。"

### 附：三个高频追问的应答要点

| 追问 | 应答 |
|---|---|
| "这套在 ISA-95 算哪一层？" | L2 采集监控 + L3 能耗/工单业务；南向 Modbus/OPC UA 吃 L1，北向 REST/Webhook/CSV 给 L4 ERP，卡在 IT/OT 边界 |
| "和直接上 ThingsBoard/FUXA 有什么区别？" | 通用平台是物模型；我按工厂口径手写了寄存器映射、峰平谷计价、阈值+持续+滞回去抖、工单状态机，6.5MB 级单二进制交付，出问题能自己修到字节级 |
| "怎么接我们厂 ERP？" | 三条路按成本排：CSV 定时导入 → REST 拉取（ERP 定时任务调 open API）→ Webhook 事件推送；全部令牌鉴权，不开数据库 |

---

## 附录 A. 真实 API 与数据表索引（只列代码里存在的，不编造）

**内部看板 API**（`src/web.rs` 路由表，浏览器同源）：

| 方法 | 路径 | 用途 |
|---|---|---|
| GET | `/api/state` `/api/health` `/api/stream` | 全量状态 / 健康检查 / SSE 实时推送 |
| GET | `/api/history` `/api/alarms` `/api/rules` `/api/workorders` `/api/devices` `/api/audit` `/api/settings` | 历史、告警、规则、工单、设备、审计、设置 |
| POST | `/api/alarms/:id/ack` `/api/alarms/:id/close` | 告警确认 / 关闭（状态机 `active→acked→closed`） |
| POST/DELETE | `/api/rules` `/api/rules/:id/toggle` `/api/rules/:id` | 规则增 / 启停 / 删 |
| POST | `/api/workorders` `/api/workorders/:id/note` `/api/workorders/:id/status` | 工单建 / 处理留痕 / 状态流转 |
| GET | `/api/report/day` `/api/report/month` | 峰平谷日报 / 月报 |
| POST | `/api/sim/cmd` | 演示注入：`overload` / `comm_fault` / `breaker` / `clear` / `water_sp` / `water_demand`，每个动作写 `audit_logs` |
| GET/POST | `/api/audit` `/api/settings/token` `/api/webhook/test` | 审计查询 / token 查看换发 / Webhook 测试 |

**开放 API**（`X-API-Token` 鉴权，供 L3/L4/BI 消费）：

| 方法 | 路径 | 用途 |
|---|---|---|
| GET | `/api/v1/open/devices` `/latest` `/history` `/alarms` | 设备 / 最新值 / 历史 / 告警 |
| POST | `/api/v1/open/ingest` | 第三方平台/PLC 推数（南向入口） |
| GET | **`/api/v1/open/export/day`** | **CSV 日结算单，ERP 财务总账定时拉** |

**数据表**（`migrations/schema.sql`）：`devices` / `metrics`（含 v0.6.0 扩展列 `fc/dtype/endian/rw`）/ `readings` / `alarm_rules` / `alarms` / `work_orders` / `wo_notes` / `settings` / `audit_logs` / **`comm_logs`** / **`inspections`** / **`defects`** / **`switching_tickets`**。

**运行事实**：`ES_PORT` 默认 **8018**，`ES_DB` 默认 `postgresql://energy:...@127.0.0.1:5436/energy_sentinel`；schema 启动幂等执行；collector 2s 一拍；`tokio-modbus` / `serialport` 为 `--features rtu` 可选依赖（规划项）。

## 附录 B. 术语与面试自检清单

| 术语 | 一句话口径 |
|---|---|
| ISA-95 / Purdue | L0 现场 → L1 控制 → L2 SCADA → L3 MES → L4 ERP；L3.5 DMZ 是 IT/OT 分界 |
| 遥测 / 遥信 / 遥控 / 遥调 | 测模拟量 / 测开关位置 / 控分合 / 改设定值；后两者必须"选择—返校—执行"+ 审计 |
| 03H / 04H / 06H | 读保持 / 读输入 / 写单寄存器 |
| MBAP | Modbus-TCP 的 7 字节头：事务/协议/长度/单元标识 |
| CRC-16/MODBUS | 多项式 0xA001、初值 0xFFFF、低字节在前 |
| ABCD 字序 | 32 位量大端；CDAB 字交换、BADC 字节交换、DCBA 全反转 |
| 五防 | 防误分误合、防带负荷拉刀闸、防带电挂地线、防带地线合闸、防误入带电间隔 |
| LOTO | Lockout/Tagout 挂牌上锁；顺序 停电→验电→挂牌→放电→接地 |
| 峰平谷 / 需量 | 分时电价分桶计费；储能靠谷充峰放套利 |
| 通信看门狗 | 15s 无新鲜数据 + `comm=false` → `alarm.comm` CRITICAL；恢复自动消警 |

**上台前自检**：

- [ ] 能默画五层拓扑与 VLAN10/20/30 子网、8018/5436/502/4840 端口
- [ ] 能背 `01 03 20 00 00 02 CF CB` 每个字节、能现场复算 CRC16
- [ ] 能说清 ABCD 四种字序怎么试、Int16 补码别当无符号
- [ ] 能背 380.5V ↔ 0x0EDD ↔ 0x2000 ↔ ×0.1 这条换算链
- [ ] 能背断线闭环：3 次重试 → 15s 看门狗 → 冻结置灰 → auto-heal
- [ ] 能背五防 5 条 + LOTO 5 步顺序 + 危急/严重/一般 24h/72h/月度
- [ ] 能说清"光储收益是冲减成本不是收入"、报表口径为何隔离 `pv_ess`/`water`
- [ ] 主动说明 sim / rtu 规划项边界，不把规划说成已上线

---

> **结语**：这份手册里每一个寄存器地址、每一帧报文、每一条种子记录、每一个 API 路径，都能在 `migrations/schema.sql`、`src/*.rs`、`docs/*.md`、`README.md` 里回溯到出处。面试时我不靠 PPT 架构图说服人，靠的是能把 **380.5V 一路追到 RS485 的 A+/B− 两根线、再一路追到 ERP 的一张记账凭证**。
