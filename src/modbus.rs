//! 真实 Modbus 采集（rtu/tcp 驱动）——双源架构的"实测"侧。
//! 严实原则：只读 devices.driver IN ('rtu','tcp') 且 enabled 的设备，
//! 点位来自 metrics 表（reg_addr/scale/fc/dtype），与 sim 永不交叉。
//! 采集失败 = comm=false 冻结标记（诚实 OFFLINE），绝不拿仿真数顶替。
//!
//! M1（沙盘）：Modbus TCP 到 OpenPLC 软 PLC（:5020）。
//! M2（硬件日）：driver='rtu' 的 RS485 串口轮询（serialport），TCP 侧不变。

use crate::db::Db;
use crate::sim::Snap;

/// 单个点位采集计划
#[derive(Clone, Debug)]
#[cfg_attr(not(feature = "rtu"), allow(dead_code))]
pub struct Pt {
    pub metric: String,
    pub addr: u16,
    pub len: u16,
    pub scale: f64,
    pub fc: String,   // 03H 保持寄存器 | 04H 输入寄存器
    pub dtype: String, // UInt16 | Int16 | UInt32
}

/// 一台设备的连接与点位计划
#[derive(Clone, Debug)]
#[cfg_attr(not(feature = "rtu"), allow(dead_code))]
pub struct DevPlan {
    pub id: String,
    pub host: String,
    pub port: u16,
    pub unit: u8,
    pub points: Vec<Pt>,
}

/// 从 devices.meta + metrics 表装载采集计划（纯 SQL，恒编译；每分钟随 sources 一起刷新）
pub async fn load_plans(
    db: &Db,
    sources: &std::collections::HashMap<String, String>,
) -> Vec<DevPlan> {
    let rtu_ids: Vec<String> = sources
        .iter()
        .filter(|(_, d)| d.as_str() == "rtu" || d.as_str() == "tcp")
        .map(|(id, _)| id.clone())
        .collect();
    if rtu_ids.is_empty() {
        return Vec::new();
    }
    let c = db.lock().await;
    let devs = c
        .query(
            "SELECT id, meta FROM devices WHERE enabled AND driver IN ('rtu','tcp')",
            &[],
        )
        .await
        .unwrap_or_default();
    let pts = c
        .query(
            "SELECT device_id, metric, scale, reg_addr, reg_len, COALESCE(fc,'03H'), COALESCE(dtype,'UInt16') \
             FROM metrics WHERE reg_addr IS NOT NULL ORDER BY device_id, reg_addr",
            &[],
        )
        .await
        .unwrap_or_default();
    drop(c);

    let mut plans: Vec<DevPlan> = Vec::new();
    for r in devs {
        let id: String = r.get(0);
        if !rtu_ids.contains(&id) {
            continue;
        }
        let meta: serde_json::Value = r.get::<_, Option<serde_json::Value>>(1).unwrap_or(json0());
        // 连接参数：meta.host 优先；兼容 meta.ip 形如 "192.168.20.21:502"
        let (mut host, mut port) = (meta["host"].as_str().unwrap_or("").to_string(), 0u16);
        if let Some(ip) = meta["ip"].as_str() {
            if let Some((h, p)) = ip.rsplit_once(':') {
                if host.is_empty() { host = h.to_string(); }
                if port == 0 { port = p.parse().unwrap_or(502); }
            }
        }
        if host.is_empty() { host = "127.0.0.1".into(); }
        if port == 0 {
            port = meta["port"].as_u64().unwrap_or(502) as u16;
        }
        let unit = meta["slave"].as_u64().unwrap_or(1).min(255) as u8;
        let points: Vec<Pt> = pts
            .iter()
            .filter(|p| p.get::<_, String>(0) == id)
            .filter_map(|p| {
                let addr: Option<i32> = p.get(3);
                addr.map(|a| Pt {
                    metric: p.get::<_, String>(1),
                    addr: a.max(0) as u16,
                    len: (p.get::<_, i32>(4).max(1)) as u16,
                    scale: p.get::<_, f64>(2),
                    fc: p.get::<_, String>(5),
                    dtype: p.get::<_, String>(6),
                })
            })
            .collect();
        plans.push(DevPlan { id, host, port, unit, points });
    }
    plans
}

fn json0() -> serde_json::Value {
    serde_json::Value::Object(Default::default())
}

// ===================== 轮询实现（feature = rtu） =====================
// OpenPLC 的 Modbus TCP 服务是单客户端：后来的连接会踢掉先前的。
// 故轮询器持有持久连接（App.plc 槽位），失败即断开重连；外部脚本偶发接入
// 最多造成一拍轮询失败（诚实 OFFLINE 一拍），下一拍自愈。
#[cfg(feature = "rtu")]
pub async fn poll(
    plan: &DevPlan,
    slot: &tokio::sync::Mutex<Option<tokio_modbus::client::Context>>,
) -> Snap {
    use tokio_modbus::prelude::*;
    let ts = chrono::Utc::now().timestamp_millis();

    let mut guard = slot.lock().await;
    if guard.is_none() {
        let addr = format!("{}:{}", plan.host, plan.port);
        let socket_addr: std::net::SocketAddr = match addr.parse() {
            Ok(a) => a,
            Err(e) => {
                eprintln!("[modbus] {} bad addr {addr}: {e}", plan.id);
                return dead_snap(plan, ts);
            }
        };
        match tokio_modbus::client::tcp::connect_slave(socket_addr, plan.unit.into()).await {
            Ok(c) => {
                eprintln!("[modbus] {} connected {addr}", plan.id);
                *guard = Some(c);
            }
            Err(e) => {
                eprintln!("[modbus] {} connect {addr} failed: {e}", plan.id);
                return dead_snap(plan, ts);
            }
        }
    }
    let ctx = guard.as_mut().expect("连接槽位已建立");

    let mut snap = Snap {
        device: plan.id.clone(),
        ts,
        u: 0.0, i: 0.0, p: 0.0, q: 0.0, pf: 0.0, f: 0.0, epi: 0.0,
        breaker: true, // 默认合闸；设备在 metrics 里声明 breaker 点位时由寄存器驱动
        comm: true,
        extra: Default::default(),
    };
    let mut ok_any = false;
    for pt in &plan.points {
        let req = if pt.fc == "04H" {
            Request::ReadInputRegisters(pt.addr, pt.len)
        } else {
            Request::ReadHoldingRegisters(pt.addr, pt.len)
        };
        let rsp = match ctx.call(req).await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[modbus] {} read {}@0x{:04X} failed: {e} → 重连", plan.id, pt.metric, pt.addr);
                *guard = None; // 连接失效：断开，下一拍重连
                return dead_snap(plan, ts);
            }
        };
        let regs: Vec<u16> = match rsp {
            Response::ReadHoldingRegisters(w) | Response::ReadInputRegisters(w) => w,
            _ => {
                *guard = None;
                return dead_snap(plan, ts);
            }
        };
        if let Some(raw) = decode(&pt.dtype, &regs) {
            let v = raw * pt.scale;
            ok_any = true;
            match pt.metric.as_str() {
                "u" => snap.u = round3(v),
                "i" => snap.i = round3(v),
                "p" => snap.p = round3(v),
                "q" => snap.q = round3(v),
                "pf" => snap.pf = round3(v),
                "f" => snap.f = round3(v),
                "epi" => snap.epi = round3(v),
                "breaker" => snap.breaker = raw != 0.0,
                k => { snap.extra.insert(k.to_string(), round3(v)); }
            }
        }
    }
    if !ok_any {
        // 一条点位都没读到：连接虽通但数据可疑，按 OFFLINE 处理
        return dead_snap(plan, ts);
    }
    snap
}

#[cfg(feature = "rtu")]
fn decode(dtype: &str, regs: &[u16]) -> Option<f64> {
    match dtype {
        "Int16" => regs.first().map(|&r| r as i16 as f64),
        "UInt32" if regs.len() >= 2 => Some((((regs[0] as u32) << 16) | regs[1] as u32) as f64),
        _ => regs.first().map(|&r| r as f64), // UInt16 与未知类型按无符号 16 位
    }
}

#[cfg(feature = "rtu")]
fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// 诚实 OFFLINE 标记：连接失败 / 无有效点位时冻结（collector 未编译 rtu feature 时也用它）
pub(crate) fn dead_snap_dev(id: &str, ts: i64) -> Snap {
    Snap {
        device: id.to_string(),
        ts,
        u: 0.0, i: 0.0, p: 0.0, q: 0.0, pf: 0.0, f: 0.0, epi: 0.0,
        breaker: false,
        comm: false,
        extra: Default::default(),
    }
}

#[cfg(feature = "rtu")]
fn dead_snap(plan: &DevPlan, ts: i64) -> Snap {
    dead_snap_dev(&plan.id, ts)
}
