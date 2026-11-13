mod alarm;
mod collector;
mod db;
mod sim;
mod web;

/// EnergySentinel —— 车间配电与能源监控哨兵
/// 默认端口 8018（ES_PORT 可覆盖），数据库 ES_DB 可覆盖。
/// 硬件接入日：cargo build --release --features rtu，并把 devices.driver 改为 rtu。
#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("ES_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8018);
    let db_url = std::env::var("ES_DB").unwrap_or_else(|_| {
        "postgresql://energy:energy2026@127.0.0.1:5436/energy_sentinel".to_string()
    });

    let db = db::connect(&db_url).await.expect("postgres 连接失败（energy-pg 容器在 5436）");
    let token = db::ensure_token(&db).await;
    println!("[boot] schema ready, api_token={token}");

    let app = collector::build_app(db);
    tokio::spawn(collector::run(app.clone()));

    let router = web::router(app);
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port))
        .await
        .expect("端口被占用");
    println!("[boot] EnergySentinel http://0.0.0.0:{port}");
    axum::serve(listener, router).await.unwrap();
}
