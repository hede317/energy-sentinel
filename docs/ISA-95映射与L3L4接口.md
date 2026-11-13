# EnergySentinel × ISA-95：分层定位与 L3/L4 接口

> 用途：面试作品集。一句话——**我做的系统卡在 L2（SCADA/监控）与 L3（MES/能管）之间，南向吃 L1/L2 数据（Modbus/OPC UA），北向用开放 API 对接 L4（ERP 计费/总账）**。

## 一、ISA-95 分层图（本系统视角）

```
L4  ERP / 经营层      NetSuite·ERPNext·用友 —— 订单/总账/成本核算
      ▲   ▲            对接方式：REST 开放 API + CSV 导出（能耗电费入账、工单关单回写）
      │   │
L3  MES / 运营层      工单闭环·能耗报表(峰平谷计价)·告警处置 —— EnergySentinel 业务层
      ▲               对接方式：work_orders / alarms 表 + Webhook 事件推送
      │
L2  SCADA / 监控层    采集·一次接线图·实时趋势·越限判断 —— EnergySentinel 采集与看板
      ▲               （对标 FUXA；告警引擎的"阈值+持续+滞回"就是 SCADA 报警学）
      │
L1  控制 / 仪表层     汇川 PLC·变频器·安科瑞 ADL400 电表 —— Modbus-RTU / OPC UA
      ▲
      │
L0  现场设备层        电机·泵·传感器·断路器（遥信遥测源头）
```

## 二、数据模型 → ISA-95 概念映射

| 本系统表/对象 | ISA-95 概念 | 说明 |
|---|---|---|
| `devices` + `metrics`（寄存器/节点映射配置化） | B2MML EquipmentCapability / 设备模型 | 加设备=插表，不改代码 |
| `readings`（P/U/I/PF/压力/频率…） | Process Data / 遥测 telemetry | 10s 周期，31 天滚动 |
| `alarms`（active→acked→closed） | L2 报警事件 → L3 事件管理 | 滞回+持续时间=报警去抖 |
| `work_orders` + `wo_notes` | L3 维护工单（MES 维护域最小闭环） | 告警一键转工单、处理留痕、关单归档 |
| 能耗日报（峰/平/谷 + 电费） | L3 能源管理 → L4 成本核算的接口数据 | CSV/REST 供 ERP 总账入账 |
| `/api/v1/open/ingest` | 南向集成入口（L1/L2 → 本系统） | 第三方传感器/PLC/平台推数即接入 |
| `/api/v1/open/latest·history·alarms` | 北向数据接口（本系统 → L3/L4/BI） | 令牌鉴权、JSON、CORS 开放 |
| Webhook（alarm.triggered/closed） | 事件驱动集成（→ 消息平台/工单系统） | TG/钉钉/企业微信可自建接手 |

## 三、与既有资产的 L3/L4 对接清单

| 既有系统 | 对接点 | 状态 |
|---|---|---|
| eyou 工作台（矿山 14 系统中台） | EnergySentinel 作为新工具格挂入左栏，告警走工作台审批流 | 待挂格（五处接入清单见运维记忆） |
| 矿山 ERPNext v16（deyou-mine） | 能耗电费月账 → ERP 应付/成本中心；工单关单回写 ERP 维护模块 | 接口已备（REST+CSV），脚本待写 |
| 纺织 MES（工票/报工） | 工单体系打通：设备告警工单 ↔ 报工绩效 | 规划 |
| NetSuite（ERP 迁移项目） | 生产/工单/设备/能耗数据模型 → ISA-95 L3/L4 接口方案 | **素材待补**：拿到 NetSuite 数据字典后，把 PurchaseOrder/WorkOrder/Item 表与本系统 devices/work_orders 对齐，产出映射矩阵 |

## 四、面试问答三条（背熟）

1. **"你这套东西在工厂 IT 架构里算哪一层？"**
   L2 采集监控 + L3 的能耗/工单业务。南向 Modbus/OPC UA 吃 L1 数据，北向 REST/Webhook 给 L4 ERP 供数——IT/OT 边界上那块最缺人的位置。

2. **"和直接上 ThingsBoard 有什么区别？"**
   平台是通用物模型，我是按"配电监控+能管+工单闭环"的工厂口径亲手建的：寄存器映射、峰平谷计价、报警去抖（阈值+持续+滞回）、工单状态机都是我一行行写的，6.5MB 单二进制交付，这正好说明我懂 L2/L3 每一环的原理，出问题能修。

3. **"怎么接进我们厂的 ERP？"**
   三条路按成本排：CSV 定期导入（电费月账）→ REST 拉取（ERP 侧定时任务调 open API）→ Webhook 事件推送（告警/关单实时进 ERP 工作流）。都有令牌鉴权，不用开数据库。

## 五、90 天行动对照（来源：导师清单）

| 清单 | 本系统对应 |
|---|---|
| PLC 小项目（恒压供水）→ OPC UA → 数据库 → Web 看板 | ✅ 供水实验台（闭环仿真）+ OPC UA 桥（scripts/opcua_bridge.py，配演示服务器零硬件跑通） |
| ERP 数据模型 → ISA-95 L3/L4 接口方案 | ✅ 本文档（NetSuite 素材到位后补映射矩阵） |
| 低压识图与安全 | docs/硬件接入 §识图与安全清单 |
