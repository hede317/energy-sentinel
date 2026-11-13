use crate::db::Db;
use serde_json::json;
use tokio::sync::broadcast::Sender;

#[derive(Clone, Debug)]
pub struct Rule {
    pub id: i32,
    pub device_id: String,
    pub metric: String,
    pub op: String,
    pub threshold: f64,
    pub duration_s: i32,
    pub hysteresis: f64,
    pub severity: String,
    pub name_key: String,
    pub name_en: String,
    pub enabled: bool,
}

/// 最新值缓存（collector 维护）
pub type Latest = std::collections::HashMap<String, crate::collector::DevState>;

pub struct Engine {
    pub rules: Vec<Rule>,
    pending: std::collections::HashMap<i32, i64>,   // rule_id -> cond 持续起始 epoch s
    fired: std::collections::HashMap<i32, i32>,     // rule_id -> 活跃 alarm id
    comm_fired: std::collections::HashMap<String, i32>,
    pub last_reload: i64,
}

pub async fn load_rules(db: &Db) -> Vec<Rule> {
    let c = db.lock().await;
    let rows = c
        .query(
            "SELECT id,device_id,metric,op,threshold,duration_s,hysteresis,severity,name_key,name_en,enabled \
             FROM alarm_rules ORDER BY id",
            &[],
        )
        .await
        .unwrap_or_default();
    drop(c);
    rows.iter()
        .map(|r| Rule {
            id: r.get(0),
            device_id: r.get(1),
            metric: r.get(2),
            op: r.get(3),
            threshold: r.get(4),
            duration_s: r.get(5),
            hysteresis: r.get(6),
            severity: r.get(7),
            name_key: r.get(8),
            name_en: r.get(9),
            enabled: r.get(10),
        })
        .collect()
}

impl Engine {
    pub fn new() -> Self {
        Self { rules: vec![], pending: Default::default(), fired: Default::default(), comm_fired: Default::default(), last_reload: 0 }
    }

    pub async fn reload_if_stale(&mut self, db: &Db, force: bool) {
        let now = chrono::Utc::now().timestamp();
        if force || now - self.last_reload > 30 {
            self.rules = load_rules(db).await;
            self.last_reload = now;
        }
    }

    fn cond(&self, r: &Rule, v: f64) -> bool {
        if r.op == "lt" { v < r.threshold } else { v > r.threshold }
    }
    fn cleared(&self, r: &Rule, v: f64) -> bool {
        if r.op == "lt" { v > r.threshold + r.hysteresis } else { v < r.threshold - r.hysteresis }
    }

    pub async fn evaluate(&mut self, db: &Db, latest: &Latest, tx: &Sender<serde_json::Value>, webhook: &str) {
        let now = chrono::Utc::now().timestamp();
        let live_ids: std::collections::HashSet<i32> = self.rules.iter().map(|r| r.id).collect();
        self.fired.retain(|k, _| live_ids.contains(k));
        self.pending.retain(|k, _| live_ids.contains(k));

        let rules = self.rules.clone();
        for r in rules.iter().filter(|r| r.enabled) {
            let Some(dev) = latest.get(&r.device_id) else { continue };
            if !dev.comm { continue; }
            let Some(v) = dev.values.get(&r.metric) else { continue };

            if self.cond(r, *v) {
                let since = *self.pending.entry(r.id).or_insert(now);
                if now - since >= r.duration_s as i64 && !self.fired.contains_key(&r.id) {
                    let aid = create_alarm(
                        db, tx, webhook,
                        Some(r.id), &r.device_id, &r.metric, &r.severity,
                        &r.name_key, &r.name_en, *v, r.threshold,
                    ).await;
                    self.fired.insert(r.id, aid);
                }
            } else {
                self.pending.remove(&r.id);
                if self.cleared(r, *v) {
                    if let Some(aid) = self.fired.remove(&r.id) {
                        close_alarm(db, tx, webhook, aid).await;
                    }
                }
            }
        }

        // 通信看门狗：15s 无新鲜数据 → 通信中断告警
        for (dev_id, dev) in latest.iter() {
            let fresh = now * 1000 - dev.ts < 15_000;
            if !fresh && !dev.comm {
                if !self.comm_fired.contains_key(dev_id) {
                    let aid = create_alarm(
                        db, tx, webhook, None, dev_id, "comm", "critical",
                        "alarm.comm", "Communication lost", 0.0, 0.0,
                    ).await;
                    self.comm_fired.insert(dev_id.clone(), aid);
                }
            } else if self.comm_fired.contains_key(dev_id) {
                if let Some(aid) = self.comm_fired.remove(dev_id) {
                    close_alarm(db, tx, webhook, aid).await;
                }
            }
        }
    }
}

fn alarm_json(
    id: i32, rule_id: Option<i32>, device: &str, metric: &str, severity: &str,
    message: &str, meta: serde_json::Value, value: f64, status: &str,
    triggered_at: chrono::DateTime<chrono::Utc>, closed_at: Option<chrono::DateTime<chrono::Utc>>,
) -> serde_json::Value {
    json!({
        "id": id, "rule_id": rule_id, "device_id": device, "metric": metric,
        "severity": severity, "message": message, "meta": meta, "value": round2(value),
        "status": status, "triggered_at": triggered_at.to_rfc3339(),
        "closed_at": closed_at.map(|t| t.to_rfc3339()),
    })
}

fn round2(v: f64) -> f64 { (v * 100.0).round() / 100.0 }

#[allow(clippy::too_many_arguments)]
pub async fn create_alarm(
    db: &Db, tx: &Sender<serde_json::Value>, webhook: &str,
    rule_id: Option<i32>, device: &str, metric: &str, severity: &str,
    name_key: &str, name_en: &str, value: f64, threshold: f64,
) -> i32 {
    let message = format!("{name_en} [{device}] value={:.1}", round2(value));
    let meta = json!({"key": name_key, "params": {"device": device, "value": round2(value), "threshold": round2(threshold)}});
    let c = db.lock().await;
    let row = c
        .query_one(
            "INSERT INTO alarms(rule_id,device_id,metric,severity,message,meta,value,status,triggered_at) \
             VALUES($1,$2,$3,$4,$5,$6,$7,'active',now()) RETURNING id,triggered_at",
            &[&rule_id, &device, &metric, &severity, &message, &meta, &value],
        )
        .await;
    drop(c);
    match row {
        Ok(r) => {
            let id: i32 = r.get(0);
            let trig: chrono::DateTime<chrono::Utc> = r.get(1);
            let a = alarm_json(id, rule_id, device, metric, severity, &message, meta, value, "active", trig, None);
            let _ = tx.send(json!({"type": "alarm", "event": "triggered", "alarm": a.clone()}));
            if !webhook.is_empty() {
                fire_webhook(webhook, json!({"event": "alarm.triggered", "alarm": a}));
            }
            id
        }
        Err(e) => { eprintln!("[alarm] insert failed: {e}"); -1 }
    }
}

pub async fn close_alarm(db: &Db, tx: &Sender<serde_json::Value>, webhook: &str, id: i32) {
    let c = db.lock().await;
    let row = c
        .query_opt(
            "UPDATE alarms SET status='closed', closed_at=now() \
             WHERE id=$1 AND status IN ('active','acked') RETURNING id,device_id,severity,message",
            &[&id],
        )
        .await
        .ok()
        .flatten();
    drop(c);
    if let Some(r) = row {
        let did: String = r.get(1);
        let sev: String = r.get(2);
        let msg: String = r.get(3);
        let a = json!({"id": id, "device_id": did, "severity": sev, "message": msg, "status": "closed"});
        let _ = tx.send(json!({"type": "alarm", "event": "closed", "alarm": a.clone()}));
        if !webhook.is_empty() {
            fire_webhook(webhook, json!({"event": "alarm.closed", "alarm": a}));
        }
    }
}

pub fn fire_webhook(url: &str, payload: serde_json::Value) {
    let url = url.to_string();
    tokio::spawn(async move {
        let client = match reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(8))
            .build()
        {
            Ok(c) => c,
            Err(_) => return,
        };
        if let Err(e) = client.post(&url).json(&payload).send().await {
            eprintln!("[webhook] {e}");
        }
    });
}
