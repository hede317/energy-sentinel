use crate::alarm::{Engine, Latest};
use crate::db::{self, Db};
use crate::sim::SimWorld;
use serde_json::json;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::RwLock;
use tokio::sync::{broadcast, Mutex};

/// 每设备最新值（values: metric -> value）
#[derive(Clone, Debug, Default)]
pub struct DevState {
    pub ts: i64,
    pub comm: bool,
    pub values: HashMap<String, f64>,
}

pub struct App {
    pub db: Db,
    pub tx: broadcast::Sender<serde_json::Value>,
    pub latest: RwLock<Latest>,
    pub sim: Mutex<SimWorld>,
    pub rules_dirty: AtomicBool,
    pub tou_cache: std::sync::Mutex<(i64, crate::db::Tou)>, // (缓存时间戳, TOU配置)
}

pub type SharedApp = Arc<App>;

pub fn build_app(db: Db) -> SharedApp {
    let (tx, _) = broadcast::channel(256);
    Arc::new(App {
        db,
        tx,
        latest: RwLock::new(HashMap::new()),
        sim: Mutex::new(SimWorld::new()),
        rules_dirty: AtomicBool::new(false),
        tou_cache: std::sync::Mutex::new((0, crate::db::Tou {
            peak: vec![(8, 11), (17, 21)],
            flat: vec![(11, 13), (21, 23)],
            valley: vec![(23, 8)],
            tariff: [("peak", 1.0), ("flat", 0.62), ("valley", 0.32)]
                .into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
            currency: "CNY".into(),
        })),
    })
}

/// TOU 配置带缓存（60s），避免每 tick 打一次 settings 表
pub async fn tou_cached(app: &SharedApp) -> crate::db::Tou {
    {
        let c = app.tou_cache.lock().unwrap();
        if chrono::Utc::now().timestamp() - c.0 < 60 {
            return c.1.clone();
        }
    }
    let tou = db::get_tou(&app.db).await;
    *app.tou_cache.lock().unwrap() = (chrono::Utc::now().timestamp(), tou.clone());
    tou
}

/// 主循环：2s 一拍 —— 仿真推进 → 最新值 → 告警评估 → SSE 广播 → 周期落库
pub async fn run(app: SharedApp) {
    let mut engine = Engine::new();
    let mut tick: u64 = 0;
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    // 数据保留：31 天
    {
        let app2 = app.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(6 * 3600)).await;
                let c = app2.db.lock().await;
                let _ = c
                    .execute("DELETE FROM readings WHERE ts < now() - interval '31 days'", &[])
                    .await;
                drop(c);
            }
        });
    }

    loop {
        interval.tick().await;
        tick += 1;
        let snaps = app.sim.lock().await.tick(2.0);

        // 更新最新值（通信故障：整台冻结，等看门狗报警）
        {
            let mut lat = app.latest.write().unwrap();
            for s in &snaps {
                let entry = lat.entry(s.device.clone()).or_default();
                if s.comm {
                    entry.ts = s.ts;
                    entry.comm = true;
                    let v = &mut entry.values;
                    v.insert("u".into(), s.u);
                    v.insert("i".into(), s.i);
                    v.insert("p".into(), s.p);
                    v.insert("q".into(), s.q);
                    v.insert("pf".into(), s.pf);
                    v.insert("f".into(), s.f);
                    v.insert("epi".into(), s.epi);
                    // 场景扩展测点（供水 pressure/flow/sp 等）
                    for (k, val) in &s.extra {
                        v.insert(k.clone(), *val);
                    }
                    // 遥信合闸状态存 values 里，方便规则引用
                    v.insert("breaker".into(), if s.breaker { 1.0 } else { 0.0 });
                } else {
                    entry.comm = false;
                }
            }
        }

        // 告警评估
        let dirty = app.rules_dirty.swap(false, Ordering::Relaxed);
        engine.reload_if_stale(&app.db, dirty).await;
        let webhook = db::get_setting(&app.db, "webhook_url").await.unwrap_or_default();
        let latest = app.latest.read().unwrap().clone();
        engine.evaluate(&app.db, &latest, &app.tx, &webhook).await;

        // SSE tick
        let devs: serde_json::Map<String, serde_json::Value> = latest
            .iter()
            .map(|(id, st)| {
                let mut m = serde_json::Map::new();
                for (k, v) in &st.values {
                    m.insert(k.clone(), json!((v * 1000.0).round() / 1000.0));
                }
                m.insert("_ts".into(), json!(st.ts));
                m.insert("_comm".into(), json!(st.comm));
                (id.clone(), serde_json::Value::Object(m))
            })
            .collect();
        let counts = alarm_counts(&app.db).await;
        let tou = tou_cached(&app).await;
        let hour = chrono::Local::now().format("%H").to_string().parse::<u32>().unwrap_or(0);
        let _ = app.tx.send(json!({
            "type": "tick", "ts": chrono::Utc::now().timestamp_millis(),
            "devices": devs, "alarm_counts": counts, "tou_rate": tou.rate_of(hour),
        }));

        // 落库：每 10s 一批 p/u/i/pf（总表加 f），每 60s 一次 epi
        if tick % 5 == 0 {
            persist(&app, &latest, false).await;
        }
        if tick % 30 == 0 {
            persist(&app, &latest, true).await;
        }
    }
}

async fn alarm_counts(db: &Db) -> serde_json::Value {
    let c = db.lock().await;
    let rows = c
        .query(
            "SELECT status, count(*) FROM alarms WHERE status IN ('active','acked') GROUP BY status",
            &[],
        )
        .await
        .unwrap_or_default();
    drop(c);
    let mut m = serde_json::Map::new();
    for r in rows {
        m.insert(r.get::<_, String>(0), json!(r.get::<_, i64>(1)));
    }
    m.insert("active".into(), m.get("active").cloned().unwrap_or(json!(0)));
    m.insert("acked".into(), m.get("acked").cloned().unwrap_or(json!(0)));
    serde_json::Value::Object(m)
}

async fn persist(app: &SharedApp, latest: &Latest, epi_only: bool) {
    let c = app.db.lock().await;
    for (dev, st) in latest {
        if !st.comm { continue; }
        let metrics: &[&str] = if epi_only {
            &["epi"]
        } else if dev == "meter-main" {
            &["p", "u", "i", "pf", "f"]
        } else if dev == "ws-pump" {
            &["p", "f", "pressure", "flow"]
        } else {
            &["p", "u", "i", "pf"]
        };
        for m in metrics {
            if let Some(v) = st.values.get(*m) {
                let _ = c
                    .execute(
                        "INSERT INTO readings(ts,device_id,metric,value) VALUES(now(),$1,$2,$3)",
                        &[dev, m, v],
                    )
                    .await;
            }
        }
    }
}
