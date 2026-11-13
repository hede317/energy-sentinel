use rand::Rng;
use serde::Serialize;

/// 一台设备一个时刻的快照
#[derive(Clone, Debug, Serialize)]
pub struct Snap {
    pub device: String,
    pub ts: i64,        // epoch ms
    pub u: f64,         // V
    pub i: f64,         // A
    pub p: f64,         // kW
    pub q: f64,         // kvar
    pub pf: f64,
    pub f: f64,         // Hz（供水场景=变频器输出频率）
    pub epi: f64,       // kWh 正向有功电能累计
    pub breaker: bool,  // 遥信：合闸
    pub comm: bool,     // 通信正常
    pub extra: std::collections::HashMap<String, f64>, // 场景扩展测点（pressure/flow/sp…）
}

pub struct SimWorld {
    start: std::time::Instant,
    epi: std::collections::HashMap<String, f64>,
    over: std::collections::HashMap<String, (f64, f64)>, // extra_kw, until_elapsed
    comm_bad_until: std::collections::HashMap<String, f64>,
    breaker_open: std::collections::HashSet<String>,
    // 恒压供水实验台状态
    w_sp: f64,          // 压力设定 MPa
    w_freq: f64,        // 变频器频率 Hz
    w_i: f64,           // PI 积分项
    w_pv: f64,          // 管网压力（一阶惯性后的显示值）
    w_demand_pct: f64,  // 用水扰动百分比
    w_demand_until: Option<f64>,
}

impl SimWorld {
    pub fn new() -> Self {
        let mut epi = std::collections::HashMap::new();
        for (id, v) in [("meter-main", 15234.6), ("feeder-ac", 8321.4), ("feeder-im", 6604.0), ("feeder-lt", 3188.2), ("feeder-it", 2105.7), ("ws-pump", 512.8)] {
            epi.insert(id.to_string(), v);
        }
        Self {
            start: std::time::Instant::now(),
            epi,
            over: Default::default(),
            comm_bad_until: Default::default(),
            breaker_open: Default::default(),
            w_sp: 0.42,
            w_freq: 38.0,
            w_i: 0.25,
            w_pv: 0.40,
            w_demand_pct: 0.0,
            w_demand_until: None,
        }
    }

    fn elapsed(&self) -> f64 { self.start.elapsed().as_secs_f64() }
    fn hour_now() -> f64 {
        use chrono::Timelike;
        let l = chrono::Local::now();
        (l.time().hour() * 3600 + l.time().minute() * 60 + l.time().second()) as f64 / 3600.0
    }

    pub fn overload(&mut self, dev: &str, extra_kw: f64, secs: f64) {
        self.over.insert(dev.to_string(), (extra_kw, self.elapsed() + secs));
    }
    pub fn comm_fault(&mut self, dev: &str, secs: f64) {
        self.comm_bad_until.insert(dev.to_string(), self.elapsed() + secs);
    }
    pub fn breaker(&mut self, dev: &str, on: bool) {
        if on { self.breaker_open.remove(dev); } else { self.breaker_open.insert(dev.to_string()); }
    }
    pub fn clear(&mut self) {
        self.over.clear();
        self.comm_bad_until.clear();
        self.breaker_open.clear();
        self.w_demand_pct = 0.0;
        self.w_demand_until = None;
    }

    pub fn water_sp(&mut self, sp: f64) {
        self.w_sp = sp.clamp(0.20, 0.55);
    }
    pub fn water_demand(&mut self, pct: f64, secs: f64) {
        self.w_demand_pct = pct;
        self.w_demand_until = Some(self.elapsed() + secs);
    }

    /// 恒压供水闭环：PI 调频 → 泵功率（相似定律 ∝f³）→ 管网压力（一阶惯性）
    fn water_tick(&mut self, dt: f64, rng: &mut impl Rng, ts: i64) -> Snap {
        let t = self.elapsed();
        if let Some(until) = self.w_demand_until {
            if t >= until {
                self.w_demand_pct = 0.0;
                self.w_demand_until = None;
            }
        }
        let demand = 40.0 * (1.0 + self.w_demand_pct) + Self::noise(rng, 2.0); // m³/h
        // 管网压力静态特性：频率抬压力、用水量拉低压力
        let pv_target = (self.w_freq / 50.0) * 0.75 - demand * 0.004;
        self.w_pv += (pv_target - self.w_pv) * 0.12; // 一阶惯性，让曲线有过渡过程
        let err = self.w_sp - self.w_pv;
        self.w_i = (self.w_i + err * dt * 0.15).clamp(-30.0, 30.0);
        self.w_freq = (50.0 * (0.5 + 0.8 * err + self.w_i)).clamp(20.0, 50.0);
        let power = 7.5 * (self.w_freq / 50.0).powi(3) + Self::noise(rng, 0.15);
        let flow = 0.9 * demand + self.w_freq / 50.0 * 8.0;
        let epi = self.epi.entry("ws-pump".into()).or_insert(500.0);
        *epi += power * dt / 3600.0;
        let mut extra = std::collections::HashMap::new();
        extra.insert("pressure".into(), (self.w_pv * 1000.0).round() / 1000.0);
        extra.insert("flow".into(), (flow * 10.0).round() / 10.0);
        extra.insert("sp".into(), self.w_sp);
        Snap {
            device: "ws-pump".into(), ts, u: 0.0, i: 0.0,
            p: (power * 10.0).round() / 10.0, q: 0.0, pf: 0.99,
            f: (self.w_freq * 10.0).round() / 10.0,
            epi: (*epi * 10.0).round() / 10.0, breaker: true, comm: self.comm_ok("ws-pump", t),
            extra,
        }
    }

    fn noise(rng: &mut impl Rng, amp: f64) -> f64 { rng.gen_range(-amp..amp) }

    /// 各分回路目标功率（kW）
    fn feeder_target(dev: &str, t: f64, h: f64, rng: &mut impl Rng) -> (f64, f64) {
        // 返回 (功率, 功率因数)
        match dev {
            // 空压机：加载/卸载锯齿，周期 120s
            "feeder-ac" => {
                let cyc = (t % 120.0) as f64;
                let base = if cyc < 60.0 { 45.0 } else { 15.0 };
                (base + Self::noise(rng, 1.5), 0.86 + Self::noise(rng, 0.01))
            }
            // 注塑机：合模+射胶 55kW(20s) → 保压 30kW(15s) → 待机 6kW(25s)，周期 60s
            "feeder-im" => {
                let cyc = (t % 60.0) as f64;
                let base = if cyc < 20.0 { 55.0 } else if cyc < 35.0 { 30.0 } else { 6.0 };
                (base + Self::noise(rng, 1.2), 0.78 + Self::noise(rng, 0.015))
            }
            // 照明+空调：白天照明 12kW，午间空调叠加，夜间仅安防 1.5kW
            "feeder-lt" => {
                let light = if (7.0..22.0).contains(&h) { 12.0 } else { 1.5 };
                let hvac = if (10.0..17.0).contains(&h) { 7.0 * (((h - 10.0) / 7.0) * std::f64::consts::PI).sin() } else { 0.0 };
                (light + hvac + Self::noise(rng, 0.4), 0.92 + Self::noise(rng, 0.01))
            }
            // 机房与办公 IT：稳载 11kW + 微日周期
            "feeder-it" => {
                let daily = 0.8 * ((h - 3.0) / 24.0 * 2.0 * std::f64::consts::PI).sin();
                (11.0 + daily + Self::noise(rng, 0.3), 0.96 + Self::noise(rng, 0.005))
            }
            _ => (0.0, 1.0),
        }
    }

    /// 推进一个 tick（dt 秒），返回全部设备快照
    pub fn tick(&mut self, dt: f64) -> Vec<Snap> {
        let mut rng = rand::thread_rng();
        let t = self.elapsed();
        let h = Self::hour_now();
        let ts = chrono::Utc::now().timestamp_millis();

        let feeders = ["feeder-ac", "feeder-im", "feeder-lt", "feeder-it"];
        let mut snaps = Vec::new();
        let mut total_p = 0.0;

        // 总母线电压：380V 慢漂移 + 高载压降
        let mut u_bus = 380.0 + 2.5 * ((t / 90.0) * std::f64::consts::PI).sin() + Self::noise(&mut rng, 0.6);

        // 先算分回路功率（含过载命令）
        let mut pw: std::collections::HashMap<String, (f64, f64)> = Default::default();
        for f in &feeders {
            let (mut p, pf) = Self::feeder_target(f, t, h, &mut rng);
            let ov = self.over.get(*f).copied();
            if let Some((extra, until)) = ov {
                if t < until { p += extra; } else { self.over.remove(*f); }
            }
            if self.breaker_open.contains(*f) { p = 0.0; }
            pw.insert(f.to_string(), (p, pf));
            total_p += p;
        }
        // 高载压降：总功率超 90kW 后每 20kW 掉 1.2V
        if total_p > 90.0 { u_bus -= (total_p - 90.0) / 20.0 * 1.2; }

        for f in &feeders {
            let (p, pf) = pw[*f];
            let u = u_bus + Self::noise(&mut rng, 0.4);
            let i = if u > 1.0 { p * 1000.0 / (1.732 * u * pf) } else { 0.0 };
            let q = p * (1.0 - pf * pf).sqrt() / pf;
            let comm = self.comm_ok(f, t);
            let brk = !self.breaker_open.contains(*f);
            let epi_val = {
                let e = self.epi.entry(f.to_string()).or_insert(0.0);
                *e += p * dt / 3600.0;
                (*e * 10.0).round() / 10.0
            };
            snaps.push(Snap {
                device: f.to_string(), ts, u: (u * 10.0).round() / 10.0, i: (i * 10.0).round() / 10.0,
                p: (p * 10.0).round() / 10.0, q: (q * 10.0).round() / 10.0,
                pf: (pf * 1000.0).round() / 1000.0, f: ((50.0 + Self::noise(&mut rng, 0.02)) * 10.0).round() / 10.0,
                epi: epi_val, breaker: brk, comm, extra: Default::default(),
            });
        }

        // 进线总表 = 分回路之和 × 线损 2%
        let p_main = total_p * 1.02;
        let u_m = u_bus + Self::noise(&mut rng, 0.3);
        let pf_main = if total_p > 1.0 {
            let qs: f64 = snaps.iter().map(|s| s.q).sum::<f64>();
            ((total_p * total_p + qs * qs).sqrt() / total_p).min(1.0)
        } else { 0.99 };
        let i_main = p_main * 1000.0 / (1.732 * u_m * pf_main.max(0.1));
        let epi = self.epi.entry("meter-main".into()).or_insert(0.0);
        *epi += p_main * dt / 3600.0;
        snaps.push(Snap {
            device: "meter-main".into(), ts, u: (u_m * 10.0).round() / 10.0,
            i: (i_main * 10.0).round() / 10.0, p: (p_main * 10.0).round() / 10.0,
            q: (p_main * (1.0 - pf_main * pf_main).sqrt() / pf_main.max(0.1) * 10.0).round() / 10.0,
            pf: (pf_main * 1000.0).round() / 1000.0, f: ((50.0 + Self::noise(&mut rng, 0.015)) * 10.0).round() / 10.0,
            epi: (*epi * 10.0).round() / 10.0, breaker: !self.breaker_open.contains("meter-main"), comm: self.comm_ok("meter-main", t),
            extra: Default::default(),
        });
        // 恒压供水实验台
        snaps.push(self.water_tick(dt, &mut rng, ts));
        snaps
    }

    fn comm_ok(&self, dev: &str, t: f64) -> bool {
        match self.comm_bad_until.get(dev) {
            Some(&until) => t >= until,
            None => true,
        }
    }
}
