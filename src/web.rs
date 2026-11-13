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
use chrono::{DateTime, Local, NaiveDate, TimeZone, Utc};
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
    !want.is_empty() && got == want
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
        json!({"type": "tick", "ts": Utc::now().timestamp_millis(), "devices": devs})
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
    Json(json!({
        "ts": Utc::now().timestamp_millis(),
        "devices": devices,
        "tou_rate": tou.rate_of(hour),
        "tou_spans": {"peak": tou.peak, "flat": tou.flat, "valley": tou.valley},
        "tariff": tou.tariff,
        "currency": tou.currency,
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
    let dur: i32 = b.get("duration_s").and_then(|v| v.as_i64()).unwrap_or(10) as i32;
    let hyst: f64 = b.get("hysteresis").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let c = app.db.lock().await;
    let ok = c
        .execute(
            "INSERT INTO alarm_rules(device_id,metric,op,threshold,duration_s,hysteresis,severity,name_key,name_en) \
             VALUES($1,$2,$3,$4,$5,$6,$7,'alarm.custom','Custom rule')",
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
    let start = Local.from_local_datetime(&day.and_hms_opt(0, 0, 0).unwrap()).unwrap().with_timezone(&Utc);
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
        .query("SELECT id,name FROM devices WHERE kind!='inbound' ORDER BY id", &[])
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
    let tou = db::get_tou(&app.db).await;
    let devs = device_names(&app).await;
    let mut days = Vec::new();
    let mut d = Local.with_ymd_and_hms(y, m, 1, 0, 0, 0).single().unwrap_or_else(Local::now).date_naive();
    let today = Local::now().date_naive();
    while d < today {
        let mut total = 0.0;
        let mut cost = 0.0;
        for (id, _) in &devs {
            let b = integrate_day(&app, id, d).await;
            total += b.total;
            cost += b.peak * tou.price("peak") + b.flat * tou.price("flat") + b.valley * tou.price("valley");
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
    let mut sim = app.sim.lock().await;
    match action {
        "overload" => {
            let kw = b.get("kw").and_then(|v| v.as_f64()).unwrap_or(40.0);
            let secs = b.get("secs").and_then(|v| v.as_f64()).unwrap_or(90.0);
            sim.overload(device, kw, secs);
        }
        "comm_fault" => {
            let secs = b.get("secs").and_then(|v| v.as_f64()).unwrap_or(30.0);
            sim.comm_fault(device, secs);
        }
        "breaker" => {
            let on = b.get("on").and_then(|v| v.as_bool()).unwrap_or(true);
            sim.breaker(device, on);
            let act = if on { "breaker_close" } else { "breaker_open" };
            let reason = b.get("reason").and_then(|v| v.as_str()).unwrap_or("");
            let user = b.get("user").and_then(|v| v.as_str()).unwrap_or("ops");
            let c = app.db.lock().await;
            let _ = c
                .execute(
                    "INSERT INTO audit_logs(username,action,device,detail,result) VALUES($1,$2,$3,$4,'ok')",
                    &[&user, &act, &device, &reason],
                )
                .await;
            drop(c);
        }
        "clear" => sim.clear(),
        "water_sp" => {
            sim.water_sp(b.get("sp").and_then(|v| v.as_f64()).unwrap_or(0.42));
        }
        "water_demand" => {
            let pct = b.get("pct").and_then(|v| v.as_f64()).unwrap_or(1.0);
            let secs = b.get("secs").and_then(|v| v.as_f64()).unwrap_or(90.0);
            sim.water_demand(pct, secs);
        }
        _ => return err("unknown action", StatusCode::BAD_REQUEST),
    }
    drop(sim);
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
