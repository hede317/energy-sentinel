use crate::collector::{SharedApp};
use crate::db;
use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{delete, get, options, post},
    Json, Router,
};
use chrono::{DateTime, Datelike, Local, NaiveDate, TimeZone, Utc};
use futures::stream::StreamExt;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::convert::Infallible;
use tokio_stream::wrappers::BroadcastStream;

pub fn router(app: SharedApp) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/app.js", get(app_js))
        .route("/style.css", get(style_css))
        .route("/i18n/:lang", get(i18n))
        .route("/api/state", get(api_state))
        .route("/api/health", get(api_health))
        .route("/api/stream", get(sse))
        .route("/api/history", get(api_history))
        .route("/api/alarms", get(api_alarms))
        .route("/api/alarms/:id/ack", post(alarm_ack))
        .route("/api/alarms/:id/close", post(alarm_close))
        .route("/api/rules", get(api_rules).post(rule_create))
        .route("/api/rules/:id/toggle", post(rule_toggle))
        .route("/api/rules/:id", delete(rule_delete))
        .route("/api/workorders", get(wo_list).post(wo_create))
        .route("/api/workorders/:id/note", post(wo_note))
        .route("/api/workorders/:id/status", post(wo_status))
        .route("/api/report/day", get(report_day))
        .route("/api/report/month", get(report_month))
        .route("/api/devices", get(api_devices))
        .route("/api/sim/cmd", post(sim_cmd))
        .route("/api/audit", get(audit_list).post(audit_post))
        .route("/api/settings", get(settings_get).post(settings_post))
        .route("/api/settings/token", get(token_show).post(token_regen))
        .route("/api/webhook/test", post(webhook_test))
        .route("/api/modbus/overview", get(modbus_overview))
        .route("/api/modbus/log", post(modbus_log))
        .route("/api/cmms/overview", get(cmms_overview))
        .route("/api/cmms/inspection", post(cmms_inspection))
        .route("/api/cmms/defect", post(cmms_defect))
        .route("/api/cmms/defect/status", post(cmms_defect_status))
        .route("/api/cmms/ticket", post(cmms_ticket))
        .route("/api/cmms/ticket/step", post(cmms_ticket_step))
        .route("/api/v1/open/devices", options(cors_opt).get(open_devices))
        .route("/api/v1/open/latest", options(cors_opt).get(open_latest))
        .route("/api/v1/open/history", options(cors_opt).get(open_history))
        .route("/api/v1/open/alarms", options(cors_opt).get(open_alarms))
        .route("/api/v1/open/ingest", options(cors_opt).post(open_ingest))
        .route("/api/v1/open/export/day", options(cors_opt).get(open_export_csv))
        .with_state(app)
}

// ---------- 静态 ----------
const INDEX_HTML: &str = include_str!("../static/index.html");
const APP_JS: &str = include_str!("../static/app.js");
const STYLE_CSS: &str = include_str!("../static/style.css");

fn i18n_of(lang: &str) -> &'static str {
    match lang {
        "en" => include_str!("../static/i18n/en.json"),
        "fr" => include_str!("../static/i18n/fr.json"),
        "pt" => include_str!("../static/i18n/pt.json"),
        "es" => include_str!("../static/i18n/es.json"),
        "ur" => include_str!("../static/i18n/ur.json"),
        "bn" => include_str!("../static/i18n/bn.json"),
        _ => include_str!("../static/i18n/zh.json"),
    }
}

async fn index() -> Response {
    html(INDEX_HTML)
}
async fn app_js() -> Response {
    ([(header::CONTENT_TYPE, "application/javascript; charset=utf-8")], APP_JS).into_response()
}
async fn style_css() -> Response {
    ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], STYLE_CSS).into_response()
}
async fn i18n(Path(lang): Path<String>) -> Response {
    ([(header::CONTENT_TYPE, "application/json; charset=utf-8")], i18n_of(&lang)).into_response()
}
fn html(body: &'static str) -> Response {
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], body).into_response()
}

// ---------- 工具 ----------
fn cors(body: Response) -> Response {
    let (mut parts, b) = body.into_parts();
    let h = &mut parts.headers;
    h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    h.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, HeaderValue::from_static("Content-Type, X-API-Token"));
    h.insert(header::ACCESS_CONTROL_ALLOW_METHODS, HeaderValue::from_static("GET, POST, OPTIONS"));
    Response::from_parts(parts, b)
}
async fn cors_opt() -> Response {
    cors(StatusCode::OK.into_response())
}

async fn check_token(app: &SharedApp, headers: &HeaderMap) -> bool {
    let want = db::ensure_token(&app.db).await;
    let got = headers
        .get("x-api-token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    !want.is_empty() && ct_eq(got.as_bytes(), want.as_bytes())
}

/// 常数时间比较（token 32 位定长，长度差异在此场景可忽略）
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() { return false; }
    a.iter().zip(b.iter()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn err(msg: &str, code: StatusCode) -> Response {
    (code, Json(json!({"error": msg}))).into_response()
}

// ---------- SSE ----------
async fn sse(
    State(app): State<SharedApp>,
) -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>> {
    let rx = app.tx.subscribe();
    let first = {
        let latest = app.latest.read().unwrap().clone();
        let devs: serde_json::Map<String, Value> = latest
            .iter()
            .map(|(id, st)| {
                let mut m = serde_json::Map::new();
                for (k, v) in &st.values {
                    m.insert(k.clone(), json!((v * 1000.0).round() / 1000.0));
                }
                m.insert("_ts".into(), json!(st.ts));
                m.insert("_comm".into(), json!(st.comm));
                (id.clone(), Value::Object(m))
            })
            .collect();
        let (injections, uptime_s) = {
            let sim = app.sim.lock().await;
            (sim.active_injections(), sim.uptime_s())
        };
        json!({
            "type": "tick", "ts": Utc::now().timestamp_millis(), "devices": devs,
            "injections": injections, "uptime_s": uptime_s,
            "db_rows": app.db_rows.load(std::sync::atomic::Ordering::Relaxed),
            "period_ms": 2000,
        })
    };
    let stream = futures::StreamExt::chain(
        futures::stream::once(async move { Ok(Event::default().json_data(first).unwrap()) }),
        BroadcastStream::new(rx).filter_map(|m| {
            futures::future::ready(match m {
                Ok(v) => Some(Ok(Event::default().json_data(v).unwrap())),
                Err(_) => None,
            })
        }),
    );
    Sse::new(stream).keep_alive(KeepAlive::default())
}

// ---------- 状态 / 历史 ----------
async fn api_state(State(app): State<SharedApp>) -> Response {
    let latest = app.latest.read().unwrap().clone();
    let tou = db::get_tou(&app.db).await;
    let hour = Local::now().format("%H").to_string().parse::<u32>().unwrap_or(0);
    let devices: serde_json::Map<String, Value> = latest
        .iter()
        .map(|(id, st)| {
            let mut m = serde_json::Map::new();
            for (k, v) in &st.values {
                m.insert(k.clone(), json!((v * 1000.0).round() / 1000.0));
            }
            m.insert("_ts".into(), json!(st.ts));
            m.insert("_comm".into(), json!(st.comm));
            (id.clone(), Value::Object(m))
        })
        .collect();
    let (injections, uptime_s) = {
        let sim = app.sim.lock().await;
        (sim.active_injections(), sim.uptime_s())
    };
    Json(json!({
        "ts": Utc::now().timestamp_millis(),
        "devices": devices,
        "tou_rate": tou.rate_of(hour),
        "tou_spans": {"peak": tou.peak, "flat": tou.flat, "valley": tou.valley},
        "tariff": tou.tariff,
        "currency": tou.currency,
        "injections": injections, "uptime_s": uptime_s,
        "db_rows": app.db_rows.load(std::sync::atomic::Ordering::Relaxed),
        "period_ms": 2000,
    }))
    .into_response()
}

/// 健康检查：K8s / 负载均衡探针 + 运行信息
async fn api_health(State(app): State<SharedApp>) -> Response {
    let (uptime_s, injections) = {
        let sim = app.sim.lock().await;
        (sim.uptime_s(), sim.active_injections())
    };
    let db_ok = {
        let c = app.db.lock().await;
        c.query_one("SELECT 1", &[]).await.is_ok()
    };
    Json(json!({
        "status": if db_ok { "ok" } else { "degraded" },
        "uptime_s": uptime_s,
        "db_ok": db_ok,
        "db_rows": app.db_rows.load(std::sync::atomic::Ordering::Relaxed),
        "period_ms": 2000,
        "active_injections": injections.len(),
    }))
    .into_response()
}

async fn api_history(
    State(app): State<SharedApp>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let devices: Vec<String> = q
        .get("devices")
        .map(|s| s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect())
        .unwrap_or_default();
    let metric = q.get("metric").cloned().unwrap_or_else(|| "p".into());
    let hours: f64 = q.get("hours").and_then(|s| s.parse().ok()).unwrap_or(24.0);
    let maxp: usize = q.get("max").and_then(|s| s.parse().ok()).unwrap_or(720);
    if devices.is_empty() { return err("devices required", StatusCode::BAD_REQUEST); }
    let from = Utc::now() - chrono::Duration::minutes((hours * 60.0) as i64);
    let c = app.db.lock().await;
    let mut series = serde_json::Map::new();
    for d in &devices {
        let rows = c
            .query(
                "SELECT ts,value FROM readings WHERE device_id=$1 AND metric=$2 AND ts>=$3 ORDER BY ts",
                &[d, &metric, &from],
            )
            .await;
        if let Ok(rows) = rows {
            let pts: Vec<(DateTime<Utc>, f64)> = rows
                .iter()
                .map(|r| (r.get(0), r.get::<_, f64>(1)))
                .collect();
            series.insert(d.clone(), json!(downsample(&pts, maxp)));
        }
    }
    drop(c);
    Json(json!({"metric": metric, "series": series})).into_response()
}

fn downsample(pts: &[(DateTime<Utc>, f64)], maxp: usize) -> Vec<(i64, f64)> {
    if pts.len() <= maxp {
        return pts.iter().map(|(t, v)| (t.timestamp_millis(), *v)).collect();
    }
    let bucket = (pts.len() + maxp - 1) / maxp;
    pts.chunks(bucket)
        .filter_map(|ch| {
            let t0 = ch.first()?.0.timestamp_millis();
            let avg = ch.iter().map(|(_, v)| *v).sum::<f64>() / ch.len() as f64;
            Some((t0, (avg * 100.0).round() / 100.0))
        })
        .collect()
}

// ---------- 告警 ----------
async fn api_alarms(
    State(app): State<SharedApp>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let status = q.get("status").cloned().unwrap_or_default();
    let limit: i64 = q.get("limit").and_then(|s| s.parse().ok()).unwrap_or(100);
    let c = app.db.lock().await;
    let rows = if status.is_empty() || status == "all" {
        c.query(
            "SELECT id,rule_id,device_id,metric,severity,message,meta,value,status,triggered_at,acked_at,closed_at \
             FROM alarms ORDER BY triggered_at DESC LIMIT $1",
            &[&limit],
        )
        .await
    } else {
        c.query(
            "SELECT id,rule_id,device_id,metric,severity,message,meta,value,status,triggered_at,acked_at,closed_at \
             FROM alarms WHERE status=$1 ORDER BY triggered_at DESC LIMIT $2",
            &[&status, &limit],
        )
        .await
    };
    drop(c);
    let list: Vec<Value> = rows
        .unwrap_or_default()
        .iter()
        .map(|r| {
            let acked: Option<DateTime<Utc>> = r.get(10);
            let closed: Option<DateTime<Utc>> = r.get(11);
            json!({
                "id": r.get::<_, i32>(0), "rule_id": r.get::<_, Option<i32>>(1),
                "device_id": r.get::<_, String>(2), "metric": r.get::<_, String>(3),
                "severity": r.get::<_, String>(4), "message": r.get::<_, String>(5),
                "meta": r.get::<_, Option<serde_json::Value>>(6).unwrap_or(json!({})),
                "value": r.get::<_, f64>(7), "status": r.get::<_, String>(8),
                "triggered_at": r.get::<_, DateTime<Utc>>(9).to_rfc3339(),
                "acked_at": acked.map(|t| t.to_rfc3339()),
                "closed_at": closed.map(|t| t.to_rfc3339()),
            })
        })
        .collect();
    Json(json!({"alarms": list})).into_response()
}

async fn alarm_ack(State(app): State<SharedApp>, Path(id): Path<i32>) -> Response {
    let c = app.db.lock().await;
    let r = c
        .query_opt(
            "UPDATE alarms SET status='acked', acked_at=now() WHERE id=$1 AND status='active' RETURNING id",
            &[&id],
        )
        .await
        .ok()
        .flatten();
    drop(c);
    if r.is_some() {
        let _ = app.tx.send(json!({"type":"alarm","event":"acked","alarm":{"id": id,"status":"acked"}}));
        Json(json!({"ok": true})).into_response()
    } else {
        err("not active", StatusCode::BAD_REQUEST)
    }
}

async fn alarm_close(State(app): State<SharedApp>, Path(id): Path<i32>) -> Response {
    let webhook = db::get_setting(&app.db, "webhook_url").await.unwrap_or_default();
    crate::alarm::close_alarm(&app.db, &app.tx, &webhook, id).await;
    Json(json!({"ok": true})).into_response()
}

// ---------- 规则 ----------
async fn api_rules(State(app): State<SharedApp>) -> Response {
    let rules = crate::alarm::load_rules(&app.db).await;
    let list: Vec<Value> = rules
        .iter()
        .map(|r| {
            json!({
                "id": r.id, "device_id": r.device_id, "metric": r.metric, "op": r.op,
                "threshold": r.threshold, "duration_s": r.duration_s, "hysteresis": r.hysteresis,
                "severity": r.severity, "name_key": r.name_key, "name_en": r.name_en, "enabled": r.enabled,
            })
        })
        .collect();
    Json(json!({"rules": list})).into_response()
}

async fn rule_create(State(app): State<SharedApp>, Json(b): Json<Value>) -> Response {
    let (Some(dev), Some(metric), Some(th)) = (
        b.get("device_id").and_then(|v| v.as_str()),
        b.get("metric").and_then(|v| v.as_str()),
        b.get("threshold").and_then(|v| v.as_f64()),
    ) else {
        return err("device_id/metric/threshold required", StatusCode::BAD_REQUEST);
    };
    let op = b.get("op").and_then(|v| v.as_str()).unwrap_or("gt").to_string();
    let sev = b.get("severity").and_then(|v| v.as_str()).unwrap_or("warning").to_string();
    let dur: i32 = b.get("duration_s").and_then(|v| v.as_i64()).unwrap_or(10).clamp(0, 3600) as i32;
    let hyst: f64 = b.get("hysteresis").and_then(|v| v.as_f64()).unwrap_or(0.0);
    if !["gt", "lt"].contains(&op.as_str()) { return err("op must be gt/lt", StatusCode::BAD_REQUEST); }
    if !["info", "warning", "critical"].contains(&sev.as_str()) { return err("bad severity", StatusCode::BAD_REQUEST); }
    let c = app.db.lock().await;
    // 同设备+测点+方向已有规则时覆盖更新（避免 uq_rule 唯一索引 500）
    let ok = c
        .execute(
            "INSERT INTO alarm_rules(device_id,metric,op,threshold,duration_s,hysteresis,severity,name_key,name_en) \
             VALUES($1,$2,$3,$4,$5,$6,$7,'alarm.custom','Custom rule') \
             ON CONFLICT (device_id,metric,op) DO UPDATE SET \
             threshold=EXCLUDED.threshold, duration_s=EXCLUDED.duration_s, \
             hysteresis=EXCLUDED.hysteresis, severity=EXCLUDED.severity, enabled=TRUE",
            &[&dev, &metric, &op, &th, &dur, &hyst, &sev],
        )
        .await
        .is_ok();
    drop(c);
    app.rules_dirty.store(true, std::sync::atomic::Ordering::Relaxed);
    if ok { Json(json!({"ok": true})).into_response() } else { err("insert failed", StatusCode::INTERNAL_SERVER_ERROR) }
}

async fn rule_toggle(State(app): State<SharedApp>, Path(id): Path<i32>) -> Response {
    let c = app.db.lock().await;
    let ok = c
        .execute("UPDATE alarm_rules SET enabled = NOT enabled WHERE id=$1", &[&id])
        .await
        .is_ok();
    drop(c);
    app.rules_dirty.store(true, std::sync::atomic::Ordering::Relaxed);
    if ok { Json(json!({"ok": true})).into_response() } else { err("failed", StatusCode::BAD_REQUEST) }
}

async fn rule_delete(State(app): State<SharedApp>, Path(id): Path<i32>) -> Response {
    let c = app.db.lock().await;
    let ok = c.execute("DELETE FROM alarm_rules WHERE id=$1", &[&id]).await.is_ok();
    drop(c);
    app.rules_dirty.store(true, std::sync::atomic::Ordering::Relaxed);
    if ok { Json(json!({"ok": true})).into_response() } else { err("failed", StatusCode::BAD_REQUEST) }
}

// ---------- 工单 ----------
async fn wo_list(State(app): State<SharedApp>) -> Response {
    let c = app.db.lock().await;
    let rows = c
        .query(
            "SELECT w.id,w.alarm_id,w.title,w.detail,w.status,w.assignee,w.created_at,w.closed_at,\
                    (SELECT json_agg(json_build_object('note',n.note,'author',n.author,'at',to_char(n.created_at,'YYYY-MM-DD HH24:MI')) ORDER BY n.id) FROM wo_notes n WHERE n.wo_id=w.id) AS notes,\
                    a.severity \
             FROM work_orders w LEFT JOIN alarms a ON a.id=w.alarm_id ORDER BY w.id DESC LIMIT 200",
            &[],
        )
        .await
        .unwrap_or_default();
    drop(c);
    let list: Vec<Value> = rows
        .iter()
        .map(|r| {
            let closed: Option<DateTime<Utc>> = r.get(7);
            json!({
                "id": r.get::<_, i32>(0), "alarm_id": r.get::<_, Option<i32>>(1),
                "title": r.get::<_, String>(2), "detail": r.get::<_, Option<String>>(3),
                "status": r.get::<_, String>(4), "assignee": r.get::<_, Option<String>>(5),
                "created_at": r.get::<_, DateTime<Utc>>(6).to_rfc3339(),
                "closed_at": closed.map(|t| t.to_rfc3339()),
                "notes": r.get::<_, Option<Value>>(8).unwrap_or(json!([])),
                "severity": r.get::<_, Option<String>>(9),
            })
        })
        .collect();
    Json(json!({"workorders": list})).into_response()
}

async fn wo_create(State(app): State<SharedApp>, Json(b): Json<Value>) -> Response {
    let title = b.get("title").and_then(|v| v.as_str()).unwrap_or("Untitled").to_string();
    let detail = b.get("detail").and_then(|v| v.as_str()).map(|s| s.to_string());
    let assignee = b.get("assignee").and_then(|v| v.as_str()).map(|s| s.to_string());
    let alarm_id = b.get("alarm_id").and_then(|v| v.as_i64()).map(|v| v as i32);
    let c = app.db.lock().await;
    let row = c
        .query_one(
            "INSERT INTO work_orders(alarm_id,title,detail,assignee) VALUES($1,$2,$3,$4) RETURNING id",
            &[&alarm_id, &title, &detail, &assignee],
        )
        .await;
    drop(c);
    match row {
        Ok(r) => Json(json!({"ok": true, "id": r.get::<_, i32>(0)})).into_response(),
        Err(e) => err(&e.to_string(), StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn wo_note(State(app): State<SharedApp>, Path(id): Path<i32>, Json(b): Json<Value>) -> Response {
    let note = b.get("note").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let author = b.get("author").and_then(|v| v.as_str()).unwrap_or("ops").to_string();
    if note.is_empty() { return err("note required", StatusCode::BAD_REQUEST); }
    let c = app.db.lock().await;
    let ok = c
        .execute("INSERT INTO wo_notes(wo_id,note,author) VALUES($1,$2,$3)", &[&id, &note, &author])
        .await
        .is_ok();
    drop(c);
    if ok { Json(json!({"ok": true})).into_response() } else { err("failed", StatusCode::BAD_REQUEST) }
}

async fn wo_status(State(app): State<SharedApp>, Path(id): Path<i32>, Json(b): Json<Value>) -> Response {
    let status = b.get("status").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if !["open", "processing", "closed"].contains(&status.as_str()) {
        return err("bad status", StatusCode::BAD_REQUEST);
    }
    let assignee = b.get("assignee").and_then(|v| v.as_str()).map(|s| s.to_string());
    let c = app.db.lock().await;
    let ok = if status == "closed" {
        c.execute(
            "UPDATE work_orders SET status='closed', closed_at=now(), assignee=COALESCE($2,assignee) WHERE id=$1",
            &[&id, &assignee],
        )
        .await
        .is_ok()
    } else {
        c.execute(
            "UPDATE work_orders SET status=$2, assignee=COALESCE($3,assignee) WHERE id=$1",
            &[&id, &status, &assignee],
        )
        .await
        .is_ok()
    };
    drop(c);
    if ok { Json(json!({"ok": true})).into_response() } else { err("failed", StatusCode::BAD_REQUEST) }
}

// ---------- 能耗报表 ----------
struct DayBuckets {
    peak: f64, flat: f64, valley: f64, total: f64,
}

async fn integrate_day(app: &SharedApp, dev: &str, day: NaiveDate) -> DayBuckets {
    // 本地日界：DST 切换日 0:00 可能不存在（LocalResult::None），旧代码 .unwrap() 会 panic 打挂请求
    let naive = day.and_hms_opt(0, 0, 0).unwrap();
    let start = match Local.from_local_datetime(&naive) {
        chrono::LocalResult::Single(dt) | chrono::LocalResult::Ambiguous(dt, _) => dt.with_timezone(&Utc),
        chrono::LocalResult::None => Utc.from_utc_datetime(&naive),
    };
    let end = start + chrono::Duration::days(1);
    let tou = db::get_tou(&app.db).await;
    let c = app.db.lock().await;
    let rows = c
        .query(
            "SELECT ts,value FROM readings WHERE device_id=$1 AND metric='p' AND ts>=$2 AND ts<$3 ORDER BY ts",
            &[&dev, &start, &end],
        )
        .await
        .unwrap_or_default();
    drop(c);
    let mut b = DayBuckets { peak: 0.0, flat: 0.0, valley: 0.0, total: 0.0 };
    let mut prev: Option<DateTime<Utc>> = None;
    for r in rows {
        let ts: DateTime<Utc> = r.get(0);
        let v: f64 = r.get(1);
        if let Some(p) = prev {
            let gap = (ts - p).num_seconds().clamp(0, 60) as f64;
            let kwh = v * gap / 3600.0;
            let h = ts.with_timezone(&Local).format("%H").to_string().parse::<u32>().unwrap_or(0);
            match tou.rate_of(h) {
                "peak" => b.peak += kwh,
                "flat" => b.flat += kwh,
                _ => b.valley += kwh,
            }
            b.total += kwh;
        }
        prev = Some(ts);
    }
    b
}

async fn device_names(app: &SharedApp) -> Vec<(String, String)> {
    let c = app.db.lock().await;
    let rows = c
        .query("SELECT id,name FROM devices WHERE kind='feeder' ORDER BY id", &[])
        .await
        .unwrap_or_default();
    drop(c);
    rows.iter().map(|r| (r.get(0), r.get(1))).collect()
}

async fn report_day(State(app): State<SharedApp>, Query(q): Query<HashMap<String, String>>) -> Response {
    let day = q
        .get("date")
        .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
        .unwrap_or_else(|| Local::now().date_naive());
    let tou = db::get_tou(&app.db).await;
    let mut devices = Vec::new();
    let mut tot = DayBuckets { peak: 0.0, flat: 0.0, valley: 0.0, total: 0.0 };
    for (id, name) in device_names(&app).await {
        let b = integrate_day(&app, &id, day).await;
        tot.total += b.total; tot.peak += b.peak; tot.flat += b.flat; tot.valley += b.valley;
        devices.push(json!({
            "id": id, "name": name, "total_kwh": r2(b.total), "peak_kwh": r2(b.peak),
            "flat_kwh": r2(b.flat), "valley_kwh": r2(b.valley),
            "cost": r2(b.peak * tou.price("peak") + b.flat * tou.price("flat") + b.valley * tou.price("valley")),
        }));
    }
    Json(json!({
        "date": day.format("%Y-%m-%d").to_string(),
        "tariff": tou.tariff, "currency": tou.currency,
        "devices": devices,
        "totals": {
            "total_kwh": r2(tot.total), "peak_kwh": r2(tot.peak), "flat_kwh": r2(tot.flat),
            "valley_kwh": r2(tot.valley),
            "cost": r2(tot.peak * tou.price("peak") + tot.flat * tou.price("flat") + tot.valley * tou.price("valley")),
        },
    }))
    .into_response()
}

async fn report_month(State(app): State<SharedApp>, Query(q): Query<HashMap<String, String>>) -> Response {
    let month = q.get("month").cloned().unwrap_or_else(|| Local::now().format("%Y-%m").to_string());
    let (y, m): (i32, u32) = {
        let p: Vec<&str> = month.split('-').collect();
        let yy = p[0].parse().unwrap_or(0);
        let mm = p.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        if !(2000..=2100).contains(&yy) || !(1..=12).contains(&mm) {
            return err("bad month (YYYY-MM)", StatusCode::BAD_REQUEST);
        }
        (yy, mm)
    };
    let tou = crate::collector::tou_cached(&app).await;
    let devs = device_names(&app).await;
    // 统一日界转换：DST 歧义/缺口时取最早候选，绝不退化成「现在」
    let day_start = |yy: i32, mm: u32, dd: u32| -> DateTime<Utc> {
        let naive = chrono::NaiveDate::from_ymd_opt(yy, mm, dd)
            .and_then(|d| d.and_hms_opt(0, 0, 0))
            .unwrap_or_else(|| Utc::now().naive_utc());
        match Local.from_local_datetime(&naive) {
            chrono::LocalResult::Single(dt) => dt.with_timezone(&Utc),
            chrono::LocalResult::Ambiguous(a, b) => a.min(b).with_timezone(&Utc),
            chrono::LocalResult::None => Utc.from_utc_datetime(&naive),
        }
    };
    let start = day_start(y, m, 1);
    let end = if m == 12 { day_start(y + 1, 1, 1) } else { day_start(y, m + 1, 1) };
    let now_cap = Utc::now().min(end);
    // 单次 SQL 拉整月 p 序列，内存按日积分（替代原 31天×设备 N+1 查询）
    let c = app.db.lock().await;
    let rows = c
        .query(
            "SELECT device_id,ts,value FROM readings WHERE metric='p' AND ts>=$1 AND ts<$2 ORDER BY ts",
            &[&start, &now_cap],
        )
        .await
        .unwrap_or_default();
    drop(c);
    let feeders: std::collections::HashSet<String> = devs.iter().map(|(id, _)| id.clone()).collect();
    let mut acc: std::collections::HashMap<(String, NaiveDate), [f64; 3]> = Default::default(); // [peak, flat, valley]
    let mut prev: std::collections::HashMap<String, (DateTime<Utc>, f64)> = Default::default();
    for r in rows {
        let dev: String = r.get(0);
        if !feeders.contains(&dev) { continue; }
        let ts: DateTime<Utc> = r.get(1);
        let v: f64 = r.get(2);
        if let Some((pt, pv)) = prev.get(&dev) {
            let gap = (ts - *pt).num_seconds().clamp(0, 60) as f64;
            let kwh = pv * gap / 3600.0;
            let h = ts.with_timezone(&Local).format("%H").to_string().parse::<u32>().unwrap_or(0);
            let day = ts.with_timezone(&Local).date_naive();
            let slot = match tou.rate_of(h) { "peak" => 0, "flat" => 1, _ => 2 };
            let cell = acc.entry((dev.clone(), day)).or_insert([0.0, 0.0, 0.0]);
            cell[slot] += kwh;
        }
        prev.insert(dev, (ts, v));
    }
    let mut days = Vec::new();
    let mut d = start.with_timezone(&Local).date_naive();
    let today = Local::now().date_naive();
    while d <= today && d.month() == m {
        let mut total = 0.0;
        let mut cost = 0.0;
        for (id, _) in &devs {
            if let Some([pk, fl, vl]) = acc.get(&(id.clone(), d)) {
                total += pk + fl + vl;
                cost += pk * tou.price("peak") + fl * tou.price("flat") + vl * tou.price("valley");
            }
        }
        days.push(json!({"date": d.format("%Y-%m-%d").to_string(), "total_kwh": r2(total), "cost": r2(cost)}));
        d += chrono::Duration::days(1);
    }
    Json(json!({"month": month, "days": days, "currency": tou.currency})).into_response()
}

fn r2(v: f64) -> f64 { (v * 100.0).round() / 100.0 }

// ---------- 设备 / 仿真控制 ----------
async fn api_devices(State(app): State<SharedApp>) -> Response {
    let c = app.db.lock().await;
    let rows = c
        .query(
            "SELECT d.id,d.name,d.kind,d.driver,d.enabled,d.meta,\
                    (SELECT json_agg(json_build_object('metric',m.metric,'unit',m.unit,'reg',to_hex(m.reg_addr::int))) FROM metrics m WHERE m.device_id=d.id) AS ms \
             FROM devices d ORDER BY d.kind DESC, d.id",
            &[],
        )
        .await
        .unwrap_or_default();
    drop(c);
    let latest = app.latest.read().unwrap().clone();
    let list: Vec<Value> = rows
        .iter()
        .map(|r| {
            let id: String = r.get(0);
            let st = latest.get(&id);
            json!({
                "id": id, "name": r.get::<_, String>(1), "kind": r.get::<_, String>(2),
                "driver": r.get::<_, String>(3), "enabled": r.get::<_, bool>(4),
                "meta": r.get::<_, Option<Value>>(5).unwrap_or(json!({})),
                "metrics": r.get::<_, Option<Value>>(6).unwrap_or(json!([])),
                "comm": st.map(|s| s.comm).unwrap_or(false),
                "last_ts": st.map(|s| s.ts).unwrap_or(0),
                "p": st.and_then(|s| s.values.get("p")).cloned().unwrap_or(0.0),
                "epi": st.and_then(|s| s.values.get("epi")).cloned().unwrap_or(0.0),
            })
        })
        .collect();
    Json(json!({"devices": list})).into_response()
}

async fn sim_cmd(State(app): State<SharedApp>, Json(b): Json<Value>) -> Response {
    let action = b.get("action").and_then(|v| v.as_str()).unwrap_or("");
    let device = b.get("device").and_then(|v| v.as_str()).unwrap_or("feeder-im");
    let user = b.get("user").and_then(|v| v.as_str()).unwrap_or("ops");
    let reason = b.get("reason").and_then(|v| v.as_str()).unwrap_or("");

    // (审计动作名, 目标设备, 明细)：每个演练/整定操作都留痕
    let mut sim = app.sim.lock().await;
    let audit: (String, String, String) = match action {
        "overload" => {
            let kw = b.get("kw").and_then(|v| v.as_f64()).unwrap_or(40.0);
            let secs = b.get("secs").and_then(|v| v.as_f64()).unwrap_or(90.0);
            sim.overload(device, kw, secs);
            ("sim_overload".into(), device.into(), format!("+{kw:.0}kW / {secs:.0}s"))
        }
        "comm_fault" => {
            let secs = b.get("secs").and_then(|v| v.as_f64()).unwrap_or(30.0);
            sim.comm_fault(device, secs);
            ("sim_comm_fault".into(), device.into(), format!("{secs:.0}s 断链"))
        }
        "breaker" => {
            let on = b.get("on").and_then(|v| v.as_bool()).unwrap_or(true);
            sim.breaker(device, on);
            let act = if on { "breaker_close" } else { "breaker_open" };
            (act.into(), device.into(), reason.to_string())
        }
        "clear" => {
            sim.clear();
            ("sim_clear".into(), "全站".into(), "清除全部故障注入".into())
        }
        "water_sp" => {
            let sp = b.get("sp").and_then(|v| v.as_f64()).unwrap_or(0.42);
            sim.water_sp(sp);
            ("water_sp".into(), "ws-pump".into(), format!("SP={sp:.2} MPa"))
        }
        "water_demand" => {
            let pct = b.get("pct").and_then(|v| v.as_f64()).unwrap_or(1.0);
            let secs = b.get("secs").and_then(|v| v.as_f64()).unwrap_or(90.0);
            sim.water_demand(pct, secs);
            ("water_demand".into(), "ws-pump".into(), format!("+{:.0}% / {secs:.0}s", pct * 100.0))
        }
        _ => return err("unknown action", StatusCode::BAD_REQUEST),
    };
    drop(sim);

    let (act, dev, detail) = audit;
    {
        let c = app.db.lock().await;
        let _ = c
            .execute(
                "INSERT INTO audit_logs(username,action,device,detail,result) VALUES($1,$2,$3,$4,'ok')",
                &[&user, &act, &dev, &detail],
            )
            .await;
        // 断链注入 / 清除恢复 → 同步落一条采集排障记录（供 Modbus 报文台展示）
        if action == "comm_fault" || action == "clear" {
            let (tx, rx, status, stage, note, lat): (&str, &str, &str, &str, &str, i32) = if action == "comm_fault" {
                ("01 03 20 00 00 02 CF CB", "(无响应)", "timeout", "Retry 3/3",
                 "注入通信故障：连续 3 次 Rx Timeout>500ms，15s 看门狗触发 OFFLINE 告警，冻结最后有效值", 520)
            } else {
                ("01 03 20 00 00 02 CF CB", "01 03 04 00 00 0E E6 7F D9", "ok", "Auto-Heal 自动恢复",
                 "清除故障注入：收到合法 RX 帧，CRC16 校验通过，链路自愈并自动消警", 18)
            };
            let _ = c.execute(
                "INSERT INTO comm_logs(device_id,proto,slave_addr,func_code,reg_addr,tx_hex,rx_hex,latency_ms,status,fault_stage,handling_note) \
                 VALUES($1,'Modbus-RTU','0x01','03H','0x2000',$2,$3,$4,$5,$6,$7)",
                &[&device, &tx, &rx, &lat, &status, &stage, &note]).await;
        }
        drop(c);
    }
    let _ = app.tx.send(json!({"type": "audit", "ts": Utc::now().timestamp_millis()}));
    Json(json!({"ok": true})).into_response()
}

// ---------- 操作审计 ----------
async fn audit_post(State(app): State<SharedApp>, Json(b): Json<Value>) -> Response {
    let action = b.get("action").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if action.is_empty() { return err("action required", StatusCode::BAD_REQUEST); }
    let user = b.get("user").and_then(|v| v.as_str()).unwrap_or("ops").to_string();
    let device = b.get("device").and_then(|v| v.as_str()).map(|x| x.to_string());
    let detail = b.get("detail").and_then(|v| v.as_str()).map(|x| x.to_string());
    let c = app.db.lock().await;
    let ok = c
        .execute(
            "INSERT INTO audit_logs(username,action,device,detail,result) VALUES($1,$2,$3,$4,'ok')",
            &[&user, &action, &device, &detail],
        )
        .await
        .is_ok();
    drop(c);
    if ok { Json(json!({"ok": true})).into_response() } else { err("failed", StatusCode::INTERNAL_SERVER_ERROR) }
}

async fn audit_list(State(app): State<SharedApp>) -> Response {
    let c = app.db.lock().await;
    let rows = c
        .query(
            "SELECT ts,username,action,device,detail,result FROM audit_logs ORDER BY id DESC LIMIT 50",
            &[],
        )
        .await
        .unwrap_or_default();
    drop(c);
    let list: Vec<Value> = rows
        .iter()
        .map(|r| {
            let ts: DateTime<Utc> = r.get(0);
            json!({
                "ts": ts.to_rfc3339(), "user": r.get::<_, Option<String>>(1),
                "action": r.get::<_, String>(2), "device": r.get::<_, Option<String>>(3),
                "detail": r.get::<_, Option<String>>(4), "result": r.get::<_, String>(5),
            })
        })
        .collect();
    Json(json!({"audit": list})).into_response()
}

// ---------- 设置 / webhook ----------
async fn settings_get(State(app): State<SharedApp>) -> Response {
    let webhook = db::get_setting(&app.db, "webhook_url").await.unwrap_or_default();
    let tou = crate::collector::tou_cached(&app).await;
    let hour = Local::now().format("%H").to_string().parse::<u32>().unwrap_or(0);
    Json(json!({
        "webhook_url": webhook,
        "tou": {"tariff": tou.tariff, "currency": tou.currency, "now_rate": tou.rate_of(hour)},
        "langs": ["zh", "en", "fr", "pt", "es", "ur", "bn"],
    }))
    .into_response()
}

async fn settings_post(State(app): State<SharedApp>, Json(b): Json<Value>) -> Response {
    if let Some(u) = b.get("webhook_url").and_then(|v| v.as_str()) {
        let u = u.trim();
        let private = ["localhost", "127.0.0.1", "0.0.0.0", "169.254.", "metadata", "//10.", "//192.168.", "//172."];
        let bad = (!u.is_empty() && !u.starts_with("https://" ) && !u.starts_with("http://"))
            || u.len() > 500
            || private.iter().any(|s| u.contains(s));
        if bad { return err("webhook_url not allowed (public http/https only)", StatusCode::BAD_REQUEST); }
        db::set_setting(&app.db, "webhook_url", u).await;
    }
    if b.get("tariff").is_some() {
        let mut tou = db::get_tou(&app.db).await;
        if let Some(p) = b["tariff"]["peak"].as_f64() { tou.tariff.insert("peak".into(), p); }
        if let Some(f) = b["tariff"]["flat"].as_f64() { tou.tariff.insert("flat".into(), f); }
        if let Some(v) = b["tariff"]["valley"].as_f64() { tou.tariff.insert("valley".into(), v); }
        let s = serde_json::to_string(&tou).unwrap_or_default();
        db::set_setting(&app.db, "tou", &s).await;
    }
    Json(json!({"ok": true})).into_response()
}

/// 查询/重新生成 token：必须已有合法 token（首启 token 在服务日志里）
async fn token_show(State(app): State<SharedApp>, headers: HeaderMap) -> Response {
    if !check_token(&app, &headers).await {
        return err("invalid token", StatusCode::UNAUTHORIZED);
    }
    let t = db::ensure_token(&app.db).await;
    Json(json!({"api_token": t})).into_response()
}

async fn token_regen(State(app): State<SharedApp>, headers: HeaderMap) -> Response {
    if !check_token(&app, &headers).await {
        return err("invalid token", StatusCode::UNAUTHORIZED);
    }
    let t = db::gen_token();
    db::set_setting(&app.db, "api_token", &t).await;
    Json(json!({"api_token": t})).into_response()
}

async fn webhook_test(State(app): State<SharedApp>) -> Response {
    let url = db::get_setting(&app.db, "webhook_url").await.unwrap_or_default();
    if url.is_empty() {
        return err("webhook_url not set", StatusCode::BAD_REQUEST);
    }
    crate::alarm::fire_webhook(
        &url,
        json!({"event": "webhook.test", "msg": "EnergySentinel webhook is alive", "ts": Utc::now().to_rfc3339()}),
    );
    Json(json!({"ok": true, "fired": url})).into_response()
}

// ---------- 开放 API ----------
async fn open_devices(State(app): State<SharedApp>, headers: HeaderMap) -> Response {
    if !check_token(&app, &headers).await {
        return err("invalid token", StatusCode::UNAUTHORIZED);
    }
    cors(api_devices(State(app)).await)
}

async fn open_latest(State(app): State<SharedApp>, headers: HeaderMap) -> Response {
    if !check_token(&app, &headers).await {
        return err("invalid token", StatusCode::UNAUTHORIZED);
    }
    cors(api_state(State(app)).await)
}

async fn open_history(
    State(app): State<SharedApp>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    if !check_token(&app, &headers).await {
        return err("invalid token", StatusCode::UNAUTHORIZED);
    }
    cors(api_history(State(app), Query(q)).await)
}

async fn open_alarms(
    State(app): State<SharedApp>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    if !check_token(&app, &headers).await {
        return err("invalid token", StatusCode::UNAUTHORIZED);
    }
    cors(api_alarms(State(app), Query(q)).await)
}

async fn open_ingest(State(app): State<SharedApp>, headers: HeaderMap, Json(b): Json<Value>) -> Response {
    if !check_token(&app, &headers).await {
        return err("invalid token", StatusCode::UNAUTHORIZED);
    }
    let (Some(dev), Some(metric), Some(value)) = (
        b.get("device").and_then(|v| v.as_str()),
        b.get("metric").and_then(|v| v.as_str()),
        b.get("value").and_then(|v| v.as_f64()),
    ) else {
        return err("device/metric/value required", StatusCode::BAD_REQUEST);
    };
    // 外部设备自动注册
    {
        let c = app.db.lock().await;
        let _ = c
            .execute(
                "INSERT INTO devices(id,name,kind,driver) VALUES($1,$1,'feeder','ext') ON CONFLICT (id) DO NOTHING",
                &[&dev],
            )
            .await;
        let _ = c
            .execute(
                "INSERT INTO readings(ts,device_id,metric,value) VALUES(now(),$1,$2,$3)",
                &[&dev, &metric, &value],
            )
            .await;
        drop(c);
    }
    let mut lat = app.latest.write().unwrap();
    let st = lat.entry(dev.to_string()).or_default();
    st.ts = Utc::now().timestamp_millis();
    st.comm = true;
    st.values.insert(metric.to_string(), value);
    drop(lat);
    Json(json!({"ok": true})).into_response()
}

async fn open_export_csv(
    State(app): State<SharedApp>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    if !check_token(&app, &headers).await {
        return err("invalid token", StatusCode::UNAUTHORIZED);
    }
    let day = q
        .get("date")
        .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
        .unwrap_or_else(|| Local::now().date_naive());
    let tou = db::get_tou(&app.db).await;
    let mut csv = String::from("device,total_kwh,peak_kwh,flat_kwh,valley_kwh,cost\n");
    for (id, _name) in device_names(&app).await {
        let b = integrate_day(&app, &id, day).await;
        let cost = b.peak * tou.price("peak") + b.flat * tou.price("flat") + b.valley * tou.price("valley");
        csv.push_str(&format!(
            "{},{:.2},{:.2},{:.2},{:.2},{:.2}\n",
            id, b.total, b.peak, b.flat, b.valley, cost
        ));
    }
    cors((
        [(header::CONTENT_TYPE, "text/csv; charset=utf-8"),
         (header::CONTENT_DISPOSITION, format!("attachment; filename=\"energy-{day}.csv\"").as_str())],
        csv,
    )
        .into_response())
}

// ===================== v0.6.0 Modbus 点表 / 报文监视 =====================
/// 真实 CRC16 (多项式 0xA001, 初值 0xFFFF)，低字节在前
fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for b in data {
        crc ^= *b as u16;
        for _ in 0..8 {
            if crc & 1 != 0 { crc = (crc >> 1) ^ 0xA001; } else { crc >>= 1; }
        }
    }
    crc
}
fn hex_frame(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")
}
/// 构造一帧 RTU 请求 / 响应预览（含真实 CRC16）
fn rtu_tx(slave: u8, fc: u8, reg: u16, qty: u16) -> String {
    let mut v = vec![slave, fc, (reg >> 8) as u8, reg as u8, (qty >> 8) as u8, qty as u8];
    let c = crc16(&v); v.push(c as u8); v.push((c >> 8) as u8);
    hex_frame(&v)
}
fn rtu_rx(slave: u8, fc: u8, raw: u32) -> String {
    let b = [(raw >> 24) as u8, (raw >> 16) as u8, (raw >> 8) as u8, raw as u8];
    let mut v = vec![slave, fc, 4, b[0], b[1], b[2], b[3]];
    let c = crc16(&v); v.push(c as u8); v.push((c >> 8) as u8);
    hex_frame(&v)
}
fn tcp_frame(unit: u8, fc: u8, reg: u16, val: u16) -> String {
    // MBAP: 事务 00 01 / 协议 00 00 / 长度 00 06 / unit；PDU: fc + addr + value
    hex_frame(&[0, 1, 0, 0, 0, 6, unit, fc, (reg >> 8) as u8, reg as u8, (val >> 8) as u8, val as u8])
}

/// 全站 9 台设备点表矩阵 + 采集/排障日志
async fn modbus_overview(State(app): State<SharedApp>) -> Response {
    let c = app.db.lock().await;
    let rows = c.query(
        "SELECT d.id,d.name,d.kind,d.driver,d.meta, \
                m.metric,m.unit,m.scale,m.reg_addr,m.reg_len,m.fc,m.dtype,m.endian,m.rw \
         FROM devices d JOIN metrics m ON m.device_id=d.id \
         ORDER BY d.kind DESC, d.id, m.reg_addr", &[]).await.unwrap_or_default();
    drop(c);
    let latest = app.latest.read().unwrap().clone();
    let mut points: Vec<Value> = Vec::new();
    for r in &rows {
        let dev: String = r.get(0);
        let name: String = r.get(1);
        let kind: String = r.get(2);
        let driver: String = r.get(3);
        let meta: Option<Value> = r.get(4);
        let metric: String = r.get(5);
        let unit: Option<String> = r.get(6);
        let scale: f64 = r.get(7);
        let reg: Option<i32> = r.get(8);
        let fc: Option<String> = r.get(10);
        let dtype: Option<String> = r.get(11);
        let endian: Option<String> = r.get(12);
        let rw: Option<String> = r.get(13);
        let reg_i = reg.unwrap_or(0);
        let eng = latest.get(&dev).and_then(|s| s.values.get(&metric).copied()).unwrap_or(0.0);
        let raw = if scale.abs() > 1e-9 { (eng / scale).round() as i64 } else { 0 };
        let fc_s = fc.clone().unwrap_or_else(|| "03H".into());
        let slave = meta.as_ref().and_then(|m| m.get("slave")).and_then(|v| v.as_i64());
        let proto = if driver == "tcp" { "Modbus-TCP :502" } else { "Modbus-RTU 9600,8,N,1" };
        let (tx, rx) = if proto.starts_with("Modbus-TCP") {
            (tcp_frame(slave.unwrap_or(1) as u8, 0x03, reg_i as u16, raw as u16),
             tcp_frame(slave.unwrap_or(1) as u8, 0x03, reg_i as u16, raw as u16))
        } else {
            (rtu_tx(slave.unwrap_or(1) as u8, if fc_s == "04H" { 0x04 } else { 0x03 }, reg_i as u16, 2),
             rtu_rx(slave.unwrap_or(1) as u8, if fc_s == "04H" { 0x04 } else { 0x03 }, raw as u32))
        };
        points.push(json!({
            "device_id": dev, "device_name": name, "kind": kind, "driver": driver,
            "meta": meta.unwrap_or(json!({})), "metric": metric, "unit": unit.unwrap_or_default(),
            "scale": scale, "reg_hex": format!("0x{reg_i:04X}"), "reg_dec": reg_i,
            "fc": fc_s, "dtype": dtype.unwrap_or_default(), "endian": endian.unwrap_or_default(),
            "rw": rw.unwrap_or_default(), "proto": proto,
            "raw_int": raw, "raw_hex": format!("0x{raw:04X}"),
            "eng_value": (eng * 100.0).round() / 100.0,
            "tx_hex": tx, "rx_hex": rx,
        }));
    }
    let c = app.db.lock().await;
    let logs = c.query(
        "SELECT ts,device_id,proto,slave_addr,func_code,reg_addr,tx_hex,rx_hex,latency_ms,status,fault_stage,handling_note \
         FROM comm_logs ORDER BY ts DESC LIMIT 40", &[]).await.unwrap_or_default();
    drop(c);
    let logs: Vec<Value> = logs.iter().map(|r| {
        let ts: DateTime<Utc> = r.get(0);
        json!({
            "ts": ts.to_rfc3339(), "device_id": r.get::<_, Option<String>>(1),
            "proto": r.get::<_, Option<String>>(2), "slave_addr": r.get::<_, Option<String>>(3),
            "func_code": r.get::<_, Option<String>>(4), "reg_addr": r.get::<_, Option<String>>(5),
            "tx_hex": r.get::<_, Option<String>>(6), "rx_hex": r.get::<_, Option<String>>(7),
            "latency_ms": r.get::<_, Option<i32>>(8).unwrap_or(0),
            "status": r.get::<_, Option<String>>(9), "fault_stage": r.get::<_, Option<String>>(10),
            "handling_note": r.get::<_, Option<String>>(11),
        })
    }).collect();
    Json(json!({"points": points, "logs": logs, "crc_note": "CRC16 poly=0xA001 init=0xFFFF low-byte-first"})).into_response()
}

/// 前端手动触发一帧轮询 / 注入一帧 CRC 误码排障记录
async fn modbus_log(State(app): State<SharedApp>, Json(b): Json<Value>) -> Response {
    let mode = b.get("mode").and_then(|v| v.as_str()).unwrap_or("poll");
    let dev = b.get("device_id").and_then(|v| v.as_str()).unwrap_or("meter-main");
    let c = app.db.lock().await;
    let (tx, rx, status, stage, note, latency) = if mode == "crc_error" {
        ("01 03 20 00 00 02 CF CB".to_string(), "01 03 04 00 00 0E E6 7F D8 (CRC 错)".to_string(),
         "crc_error", "Retry 1/3", "注入一帧 CRC 校验错误：重发请求并核对 A+/B- 极性、120Ω 终端电阻与屏蔽层单端接地", 505)
    } else {
        (rtu_tx(1, 3, 0x2000, 2), rtu_rx(1, 3, 3814),
         "ok", "轮询正常", "手动触发单帧点位轮询：03H 读 0x2000，原始 0x0EE6=3814 ×0.1=381.4V，CRC16 校验通过", 18)
    };
    let ok = c.execute(
        "INSERT INTO comm_logs(device_id,proto,slave_addr,func_code,reg_addr,tx_hex,rx_hex,latency_ms,status,fault_stage,handling_note) \
         VALUES($1,'Modbus-RTU','0x01','03H','0x2000',$2,$3,$4,$5,$6,$7)",
        &[&dev, &tx, &rx, &(latency as i32), &status, &stage, &note]).await.is_ok();
    drop(c);
    if ok { Json(json!({"ok": true, "tx_hex": tx, "rx_hex": rx, "status": status})).into_response() }
    else { err("log insert failed", StatusCode::INTERNAL_SERVER_ERROR) }
}

// ===================== v0.6.0 光储运维 CMMS =====================
async fn cmms_overview(State(app): State<SharedApp>) -> Response {
    let c = app.db.lock().await;
    let ins = c.query("SELECT id,code,area,inspector,shift,check_date,items,abnormal_count,erp_sync_status FROM inspections ORDER BY id DESC LIMIT 30", &[]).await.unwrap_or_default();
    let def = c.query("SELECT id,code,device_id,level,category,title,phenomenon,root_cause,action_taken,reporter,handler,wo_id,erp_notif_no,status,found_at,resolved_at FROM defects ORDER BY id DESC LIMIT 50", &[]).await.unwrap_or_default();
    let tks = c.query("SELECT id,ticket_no,title,task_mode,target_device,operator,guardian,duty_leader,status,steps,safety_checks,issued_at,finished_at FROM switching_tickets ORDER BY id DESC LIMIT 20", &[]).await.unwrap_or_default();
    drop(c);
    let inspections: Vec<Value> = ins.iter().map(|r| {
        let d: chrono::NaiveDate = r.get(5);
        json!({"id": r.get::<_, i32>(0), "code": r.get::<_, Option<String>>(1), "area": r.get::<_, Option<String>>(2),
               "inspector": r.get::<_, Option<String>>(3), "shift": r.get::<_, Option<String>>(4),
               "check_date": d.to_string(), "items": r.get::<_, Option<Value>>(6).unwrap_or(json!([])),
               "abnormal_count": r.get::<_, Option<i32>>(7).unwrap_or(0), "erp_sync_status": r.get::<_, Option<String>>(8)})
    }).collect();
    let defects: Vec<Value> = def.iter().map(|r| {
        let f: DateTime<Utc> = r.get(14);
        let rv: Option<DateTime<Utc>> = r.get(15);
        json!({"id": r.get::<_, i32>(0), "code": r.get::<_, Option<String>>(1), "device_id": r.get::<_, Option<String>>(2),
               "level": r.get::<_, Option<String>>(3), "category": r.get::<_, Option<String>>(4), "title": r.get::<_, Option<String>>(5),
               "phenomenon": r.get::<_, Option<String>>(6), "root_cause": r.get::<_, Option<String>>(7), "action_taken": r.get::<_, Option<String>>(8),
               "reporter": r.get::<_, Option<String>>(9), "handler": r.get::<_, Option<String>>(10),
               "wo_id": r.get::<_, Option<i32>>(11), "erp_notif_no": r.get::<_, Option<String>>(12),
               "status": r.get::<_, Option<String>>(13), "found_at": f.to_rfc3339(),
               "resolved_at": rv.map(|x| x.to_rfc3339())})
    }).collect();
    let tickets: Vec<Value> = tks.iter().map(|r| {
        let i: DateTime<Utc> = r.get(11);
        let f: Option<DateTime<Utc>> = r.get(12);
        json!({"id": r.get::<_, i32>(0), "ticket_no": r.get::<_, Option<String>>(1), "title": r.get::<_, Option<String>>(2),
               "task_mode": r.get::<_, Option<String>>(3), "target_device": r.get::<_, Option<String>>(4),
               "operator": r.get::<_, Option<String>>(5), "guardian": r.get::<_, Option<String>>(6), "duty_leader": r.get::<_, Option<String>>(7),
               "status": r.get::<_, Option<String>>(8), "steps": r.get::<_, Option<Value>>(9).unwrap_or(json!([])),
               "safety_checks": r.get::<_, Option<Value>>(10).unwrap_or(json!([])),
               "issued_at": i.to_rfc3339(), "finished_at": f.map(|x| x.to_rfc3339())})
    }).collect();
    Json(json!({
        "inspections": inspections, "defects": defects, "tickets": tickets,
        "erp_mapping": {"接口": "/api/v1/open/export/day", "凭证": "借:制造费用-电费 / 贷:应付账款-供电局",
                        "cmms": "Webhook alarm.triggered → 缺陷通知单 → 维护工单 → 备件扣减"}
    })).into_response()
}

/// 提交巡检：含异常项自动联动生成一条缺陷台账
async fn cmms_inspection(State(app): State<SharedApp>, Json(b): Json<Value>) -> Response {
    let area = b.get("area").and_then(|v| v.as_str()).unwrap_or("光伏阵列与逆变器区");
    let inspector = b.get("inspector").and_then(|v| v.as_str()).unwrap_or("ops");
    let shift = b.get("shift").and_then(|v| v.as_str()).unwrap_or("白班");
    let items = b.get("items").cloned().unwrap_or(json!([]));
    let abnormal = items.as_array().map(|a| a.iter().filter(|x| x.get("ok").and_then(|v| v.as_bool()) == Some(false)).count()).unwrap_or(0);
    let c = app.db.lock().await;
    let row = c.query_one(
        "INSERT INTO inspections(code,area,inspector,shift,items,abnormal_count,erp_sync_status) \
         VALUES($1,$2,$3,$4,$5,$6,'synced') RETURNING id",
        &[&format!("XJ-{}", Utc::now().format("%Y%m%d-%H%M%S")), &area, &inspector, &shift, &items, &(abnormal as i32)]
    ).await;
    let (ok, defect_id) = match row {
        Ok(_) => {
            let mut did: Option<i32> = None;
            if abnormal > 0 {
                let first = items.as_array().and_then(|a| a.iter().find(|x| x.get("ok").and_then(|v| v.as_bool()) == Some(false)));
                let title = first.and_then(|x| x.get("k")).and_then(|v| v.as_str()).unwrap_or("巡检异常项");
                let val = first.and_then(|x| x.get("v")).and_then(|v| v.as_str()).unwrap_or("--");
                if let Ok(dr) = c.query_one(
                    "INSERT INTO defects(code,device_id,level,category,title,phenomenon,root_cause,action_taken,reporter,handler,erp_notif_no,status) \
                     VALUES($1,$2,'minor','巡检发现', $3,$4,'待现场复核','纳入月度检修消缺',$5,$5,$6,'open') RETURNING id",
                    &[&format!("QF-{}", Utc::now().format("%Y%m%d-%H%M%S")), &"inspection", &title,
                      &format!("巡检 [{area}] 发现异常：{title} = {val}"), &inspector,
                      &format!("ERP-PM-{}", Utc::now().format("%Y%m%d-%H%M"))]).await {
                    did = Some(dr.get(0));
                }
            }
            (true, did)
        }
        Err(_) => (false, None),
    };
    drop(c);
    if ok { Json(json!({"ok": true, "abnormal_count": abnormal, "defect_id": defect_id})).into_response() }
    else { err("inspection insert failed", StatusCode::INTERNAL_SERVER_ERROR) }
}

async fn cmms_defect(State(app): State<SharedApp>, Json(b): Json<Value>) -> Response {
    let title = b.get("title").and_then(|v| v.as_str()).unwrap_or("");
    if title.is_empty() { return err("title required", StatusCode::BAD_REQUEST); }
    let dev = b.get("device_id").and_then(|v| v.as_str()).unwrap_or("meter-main");
    let level = b.get("level").and_then(|v| v.as_str()).unwrap_or("minor");
    let cat = b.get("category").and_then(|v| v.as_str()).unwrap_or("现场发现");
    let phen = b.get("phenomenon").and_then(|v| v.as_str()).unwrap_or("");
    let c = app.db.lock().await;
    let ok = c.execute(
        "INSERT INTO defects(code,device_id,level,category,title,phenomenon,root_cause,action_taken,reporter,handler,erp_notif_no,status) \
         VALUES($1,$2,$3,$4,$5,$6,'待分析','待制定消缺方案',$7,$7,$8,'open')",
        &[&format!("QF-{}", Utc::now().format("%Y%m%d-%H%M%S")), &dev, &level, &cat, &title, &phen, &"ops",
          &format!("ERP-PM-{}", Utc::now().format("%Y%m%d-%H%M"))]).await.is_ok();
    drop(c);
    if ok { Json(json!({"ok": true})).into_response() } else { err("defect insert failed", StatusCode::INTERNAL_SERVER_ERROR) }
}

/// 流转缺陷状态；转 processing 时自动建 CMMS 工单并回填 wo_id / erp_notif_no
async fn cmms_defect_status(State(app): State<SharedApp>, Json(b): Json<Value>) -> Response {
    let id = b.get("id").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    let status = b.get("status").and_then(|v| v.as_str()).unwrap_or("processing");
    if !["open", "processing", "closed"].contains(&status) { return err("bad status", StatusCode::BAD_REQUEST); }
    let c = app.db.lock().await;
    let mut wo_id: Option<i32> = None;
    if status == "processing" {
        let title: String = c.query_opt("SELECT title FROM defects WHERE id=$1", &[&id]).await.ok().flatten()
            .map(|r| r.get::<_, Option<String>>(0).unwrap_or_default()).unwrap_or_default();
        if let Ok(r) = c.query_one(
            "INSERT INTO work_orders(title,detail,status) VALUES($1,$2,'open') RETURNING id",
            &[&format!("[CMMS消缺] {title}"), &format!("由缺陷 #{id} 自动转工单")]).await {
            wo_id = Some(r.get(0));
        }
    }
    let ok = if status == "closed" {
        c.execute("UPDATE defects SET status='closed', resolved_at=now() WHERE id=$1", &[&id]).await.is_ok()
    } else {
        c.execute("UPDATE defects SET status=$2, wo_id=COALESCE($3, wo_id), erp_notif_no=COALESCE(erp_notif_no, $4) WHERE id=$1",
                  &[&id, &status, &wo_id, &format!("ERP-PM-{}", Utc::now().format("%Y%m%d-%H%M"))]).await.is_ok()
    };
    drop(c);
    if ok { Json(json!({"ok": true, "wo_id": wo_id})).into_response() }
    else { err("update failed", StatusCode::INTERNAL_SERVER_ERROR) }
}

async fn cmms_ticket(State(app): State<SharedApp>, Json(b): Json<Value>) -> Response {
    let title = b.get("title").and_then(|v| v.as_str()).unwrap_or("");
    if title.is_empty() { return err("title required", StatusCode::BAD_REQUEST); }
    let steps = b.get("steps").cloned().unwrap_or(json!([]));
    let checks = b.get("safety_checks").cloned().unwrap_or(json!([]));
    let no = format!("DZ-{}", Utc::now().format("%Y%m-%H%M%S"));
    let c = app.db.lock().await;
    let ok = c.execute(
        "INSERT INTO switching_tickets(ticket_no,title,task_mode,target_device,operator,guardian,duty_leader,status,steps,safety_checks) \
         VALUES($1,$2,$3,$4,$5,$6,$7,'in_progress',$8,$9)",
        &[&no, &title,
          &b.get("task_mode").and_then(|v| v.as_str()).unwrap_or("停电检修"),
          &b.get("target_device").and_then(|v| v.as_str()).unwrap_or("feeder-im"),
          &b.get("operator").and_then(|v| v.as_str()).unwrap_or("ops"),
          &b.get("guardian").and_then(|v| v.as_str()).unwrap_or("监护"),
          &b.get("duty_leader").and_then(|v| v.as_str()).unwrap_or("值长"),
          &steps, &checks]).await.is_ok();
    drop(c);
    if ok { Json(json!({"ok": true, "ticket_no": no})).into_response() }
    else { err("ticket insert failed", StatusCode::INTERNAL_SERVER_ERROR) }
}

/// 唱票 → 监护复诵 → 确认执行：勾选步骤并记录时间戳，全部完成自动置 completed
async fn cmms_ticket_step(State(app): State<SharedApp>, Json(b): Json<Value>) -> Response {
    let id = b.get("id").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    let n = b.get("n").and_then(|v| v.as_i64()).unwrap_or(0) as i64;
    let c = app.db.lock().await;
    let row = c.query_opt("SELECT steps FROM switching_tickets WHERE id=$1", &[&id]).await.ok().flatten();
    let Some(row) = row else { return err("ticket not found", StatusCode::NOT_FOUND); };
    let mut steps: Vec<Value> = row.get::<_, Option<Value>>(0).and_then(|v| v.as_array().cloned()).unwrap_or_default();
    let now = Utc::now().to_rfc3339();
    for s in steps.iter_mut() {
        if s.get("n").and_then(|v| v.as_i64()) == Some(n) {
            s["done"] = json!(true);
            s["at"] = json!(now);
            s["sung_by"] = json!(b.get("operator").and_then(|v| v.as_str()).unwrap_or("ops"));
            s["guardian"] = json!(b.get("guardian").and_then(|v| v.as_str()).unwrap_or("监护"));
        }
    }
    let all = steps.iter().all(|s| s.get("done").and_then(|v| v.as_bool()) == Some(true));
    let steps_v = Value::Array(steps);
    let ok = c.execute(
        "UPDATE switching_tickets SET steps=$2, status=$3, finished_at=CASE WHEN $3='completed' THEN now() ELSE finished_at END WHERE id=$1",
        &[&id, &steps_v, &if all { "completed" } else { "in_progress" }]).await.is_ok();
    drop(c);
    if ok { Json(json!({"ok": true, "completed": all})).into_response() }
    else { err("step update failed", StatusCode::INTERNAL_SERVER_ERROR) }
}
