-- EnergySentinel schema（幂等，启动时执行）
CREATE TABLE IF NOT EXISTS devices(
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  kind TEXT NOT NULL DEFAULT 'feeder',        -- inbound 进线总表 | feeder 分回路
  driver TEXT NOT NULL DEFAULT 'sim',         -- sim 仿真 | rtu 真实Modbus | ext 外部推送
  enabled BOOLEAN NOT NULL DEFAULT TRUE,
  meta JSONB NOT NULL DEFAULT '{}'
);
CREATE TABLE IF NOT EXISTS metrics(
  id SERIAL PRIMARY KEY,
  device_id TEXT NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
  metric TEXT NOT NULL,                        -- u/i/p/q/pf/f/epi
  unit TEXT,
  scale DOUBLE PRECISION NOT NULL DEFAULT 1.0,
  reg_addr INT,                                -- Modbus 保持寄存器地址（rtu 驱动用）
  reg_len INT NOT NULL DEFAULT 2,
  UNIQUE(device_id, metric)
);
CREATE TABLE IF NOT EXISTS readings(
  ts TIMESTAMPTZ NOT NULL,
  device_id TEXT NOT NULL,
  metric TEXT NOT NULL,
  value DOUBLE PRECISION NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_readings_lookup ON readings(device_id, metric, ts DESC);
CREATE TABLE IF NOT EXISTS alarm_rules(
  id SERIAL PRIMARY KEY,
  device_id TEXT NOT NULL,
  metric TEXT NOT NULL,
  op TEXT NOT NULL DEFAULT 'gt',               -- gt | lt
  threshold DOUBLE PRECISION NOT NULL,
  duration_s INT NOT NULL DEFAULT 10,
  hysteresis DOUBLE PRECISION NOT NULL DEFAULT 0,
  severity TEXT NOT NULL DEFAULT 'warning',    -- info | warning | critical
  name_key TEXT NOT NULL DEFAULT '',
  name_en TEXT NOT NULL DEFAULT '',
  enabled BOOLEAN NOT NULL DEFAULT TRUE
);
CREATE TABLE IF NOT EXISTS alarms(
  id SERIAL PRIMARY KEY,
  rule_id INT,
  device_id TEXT NOT NULL,
  metric TEXT NOT NULL,
  severity TEXT NOT NULL DEFAULT 'warning',
  message TEXT NOT NULL,
  meta JSONB NOT NULL DEFAULT '{}',
  value DOUBLE PRECISION NOT NULL DEFAULT 0,
  status TEXT NOT NULL DEFAULT 'active',       -- active | acked | closed
  triggered_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  acked_at TIMESTAMPTZ,
  closed_at TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS idx_alarms_status ON alarms(status, triggered_at DESC);
-- 规则去重（设备+测点+方向唯一，防种子重复插入）：先删重，再建唯一索引
DELETE FROM alarm_rules a USING alarm_rules b
 WHERE a.id > b.id AND a.device_id=b.device_id AND a.metric=b.metric AND a.op=b.op;
CREATE UNIQUE INDEX IF NOT EXISTS uq_rule ON alarm_rules(device_id, metric, op);
CREATE TABLE IF NOT EXISTS work_orders(
  id SERIAL PRIMARY KEY,
  alarm_id INT,
  title TEXT NOT NULL,
  detail TEXT,
  status TEXT NOT NULL DEFAULT 'open',         -- open | processing | closed
  assignee TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  closed_at TIMESTAMPTZ
);
CREATE TABLE IF NOT EXISTS wo_notes(
  id SERIAL PRIMARY KEY,
  wo_id INT NOT NULL REFERENCES work_orders(id) ON DELETE CASCADE,
  note TEXT NOT NULL,
  author TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE IF NOT EXISTS settings(
  k TEXT PRIMARY KEY,
  v TEXT NOT NULL
);
-- 遥控/遥调操作审计日志（选择—返校—执行 全程留痕）
CREATE TABLE IF NOT EXISTS audit_logs(
  id SERIAL PRIMARY KEY,
  ts TIMESTAMPTZ NOT NULL DEFAULT now(),
  username TEXT,
  action TEXT NOT NULL,
  device TEXT,
  detail TEXT,
  result TEXT NOT NULL DEFAULT 'ok'
);
-- 进线总表额定电流（负载率计算口径）
UPDATE devices SET meta = jsonb_set(meta, '{rated_i}', '400'::jsonb, true) WHERE id='meter-main' AND NOT (meta ? 'rated_i');
-- 供水实验台不计入配电能耗报表口径
INSERT INTO devices(id,name,kind,driver) VALUES('ws-pump','恒压供水泵站','water','sim') ON CONFLICT (id) DO NOTHING;
UPDATE devices SET kind='water' WHERE id='ws-pump' AND kind<>'water';

-- ===== 种子数据（幂等）=====
INSERT INTO devices(id,name,kind,driver,meta) VALUES
 ('meter-main','进线总表 ADL400','inbound','sim','{"model":"ACREL ADL400","phase":3}'),
 ('feeder-ac','1# 空压机回路','feeder','sim','{"model":"ACREL ADL400","phase":3}'),
 ('feeder-im','2# 注塑机回路','feeder','sim','{"model":"ACREL ADL400","phase":3}'),
 ('feeder-lt','3# 照明与空调回路','feeder','sim','{"model":"ACREL ADL400","phase":3}'),
 ('feeder-it','4# 机房与办公IT回路','feeder','sim','{"model":"ACREL ADL400","phase":3}'),
 ('ws-pump','恒压供水泵站','water','sim','{"scene":"water","plc":"Inovance Easy (planned)"}')
ON CONFLICT (id) DO NOTHING;

-- 寄存器映射（安科瑞 ADL400 常用点位；接真表时按实际说明书在 metrics 表调整）
INSERT INTO metrics(device_id,metric,unit,scale,reg_addr,reg_len) VALUES
 ('meter-main','u','V',0.1,0x2000,2),('meter-main','i','A',0.01,0x2002,2),
 ('meter-main','p','kW',0.1,0x2012,2),('meter-main','pf','',0.001,0x2015,2),
 ('meter-main','f','Hz',0.01,0x2017,2),('meter-main','epi','kWh',0.1,0x2100,4),
 ('feeder-ac','u','V',0.1,0x2000,2),('feeder-ac','i','A',0.01,0x2002,2),
 ('feeder-ac','p','kW',0.1,0x2012,2),('feeder-ac','pf','',0.001,0x2015,2),
 ('feeder-ac','epi','kWh',0.1,0x2100,4),
 ('feeder-im','u','V',0.1,0x2000,2),('feeder-im','i','A',0.01,0x2002,2),
 ('feeder-im','p','kW',0.1,0x2012,2),('feeder-im','pf','',0.001,0x2015,2),
 ('feeder-im','epi','kWh',0.1,0x2100,4),
 ('feeder-lt','u','V',0.1,0x2000,2),('feeder-lt','i','A',0.01,0x2002,2),
 ('feeder-lt','p','kW',0.1,0x2012,2),('feeder-lt','pf','',0.001,0x2015,2),
 ('feeder-lt','epi','kWh',0.1,0x2100,4),
 ('feeder-it','u','V',0.1,0x2000,2),('feeder-it','i','A',0.01,0x2002,2),
 ('feeder-it','p','kW',0.1,0x2012,2),('feeder-it','pf','',0.001,0x2015,2),
 ('feeder-it','epi','kWh',0.1,0x2100,4),
 ('ws-pump','pressure','MPa',1.0,NULL,2),('ws-pump','f','Hz',1.0,NULL,2),
 ('ws-pump','flow','m3/h',1.0,NULL,2),('ws-pump','p','kW',1.0,NULL,2)
ON CONFLICT (device_id,metric) DO NOTHING;

-- 规则只在空表时播种（用户删过的规则重启不复活）
INSERT INTO alarm_rules(device_id,metric,op,threshold,duration_s,hysteresis,severity,name_key,name_en)
SELECT * FROM (VALUES
 ('feeder-ac','p','gt',58.0::float8,20,3.0::float8,'critical','alarm.overload','Air compressor circuit overload'),
 ('feeder-im','p','gt',62.0,15,3.0,'critical','alarm.overload','Injection molding circuit overload'),
 ('meter-main','u','gt',400.0,10,2.0,'warning','alarm.overvoltage','Inbound overvoltage'),
 ('meter-main','u','lt',360.0,10,2.0,'warning','alarm.undervoltage','Inbound undervoltage'),
 ('feeder-im','pf','lt',0.75,60,0.02,'info','alarm.lowpf','Injection molding low power factor'),
 ('ws-pump','pressure','lt',0.38,5,0.02,'warning','alarm.waterlow','Water supply pressure low')
) AS seed(device_id,metric,op,threshold,duration_s,hysteresis,severity,name_key,name_en)
WHERE NOT EXISTS (SELECT 1 FROM alarm_rules);

INSERT INTO settings(k,v) VALUES
 ('webhook_url',''),
 ('tou','{"peak":[[8,11],[17,21]],"flat":[[11,13],[21,23]],"valley":[[23,8]],"tariff":{"peak":1.0,"flat":0.62,"valley":0.32},"currency":"CNY"}')
ON CONFLICT (k) DO NOTHING;
