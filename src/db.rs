use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_postgres::Client;

pub type Db = Arc<Mutex<Client>>;

pub const SCHEMA_SQL: &str = include_str!("../migrations/schema.sql");

pub async fn connect(url: &str) -> anyhow::Result<Db> {
    // PG 容器可能比本程序晚起，重试 30 次
    let mut last = String::new();
    for i in 0..30 {
        match tokio_postgres::connect(url, tokio_postgres::NoTls).await {
            Ok((client, conn)) => {
                tokio::spawn(async move {
                    if let Err(e) = conn.await {
                        eprintln!("[db] connection error: {e}");
                    }
                });
                let db: Db = Arc::new(Mutex::new(client));
                init_schema(&db).await?;
                return Ok(db);
            }
            Err(e) => {
                last = e.to_string();
                if i == 0 { eprintln!("[db] waiting for postgres: {e}"); }
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        }
    }
    anyhow::bail!("postgres unreachable after retries: {last}")
}

pub async fn init_schema(db: &Db) -> anyhow::Result<()> {
    let c = db.lock().await;
    c.batch_execute(SCHEMA_SQL).await?;
    Ok(())
}

pub async fn get_setting(db: &Db, k: &str) -> Option<String> {
    let c = db.lock().await;
    c.query_opt("SELECT v FROM settings WHERE k=$1", &[&k])
        .await
        .ok()
        .flatten()
        .map(|r| r.get::<_, String>(0))
}

pub async fn set_setting(db: &Db, k: &str, v: &str) {
    let c = db.lock().await;
    let _ = c
        .execute(
            "INSERT INTO settings(k,v) VALUES($1,$2) ON CONFLICT (k) DO UPDATE SET v=EXCLUDED.v",
            &[&k, &v],
        )
        .await;
}

pub fn gen_token() -> String {
    use rand::Rng;
    let mut r = rand::thread_rng();
    (0..8).map(|_| format!("{:04x}", r.gen::<u16>())).collect::<Vec<_>>().join("")
}

/// 读 API token，没有则生成
pub async fn ensure_token(db: &Db) -> String {
    if let Some(t) = get_setting(db, "api_token").await {
        if !t.is_empty() { return t; }
    }
    let t = gen_token();
    set_setting(db, "api_token", &t).await;
    t
}

/// 时段电价配置
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Tou {
    pub peak: Vec<(u32, u32)>,
    pub flat: Vec<(u32, u32)>,
    pub valley: Vec<(u32, u32)>,
    pub tariff: std::collections::BTreeMap<String, f64>,
    pub currency: String,
}

fn span_hit(spans: &[(u32, u32)], h: u32) -> bool {
    spans.iter().any(|(a, b)| if a <= b { h >= *a && h < *b } else { h >= *a || h < *b })
}

impl Tou {
    pub fn rate_of(&self, h: u32) -> &'static str {
        if span_hit(&self.peak, h) { "peak" }
        else if span_hit(&self.flat, h) { "flat" }
        else { "valley" }
    }
    pub fn price(&self, rate: &str) -> f64 { *self.tariff.get(rate).unwrap_or(&0.6) }
    pub fn symbol(&self) -> &'static str { if self.currency == "CNY" { "¥" } else { "$" } }
}

pub async fn get_tou(db: &Db) -> Tou {
    let raw = get_setting(db, "tou").await.unwrap_or_else(|| "{}".into());
    serde_json::from_str(&raw).unwrap_or(Tou {
        peak: vec![(8, 11), (17, 21)],
        flat: vec![(11, 13), (21, 23)],
        valley: vec![(23, 8)],
        tariff: [("peak", 1.0), ("flat", 0.62), ("valley", 0.32)]
            .into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
        currency: "CNY".into(),
    })
}
