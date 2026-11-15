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

-- ===================== v0.6.0 光储微网点表 + 工业运维 CMMS =====================
-- metrics 点表扩展列（功能码 / 数据类型 / 字节序 / 读写）
ALTER TABLE metrics ADD COLUMN IF NOT EXISTS fc TEXT DEFAULT '03H';
ALTER TABLE metrics ADD COLUMN IF NOT EXISTS dtype TEXT DEFAULT 'UInt16';
ALTER TABLE metrics ADD COLUMN IF NOT EXISTS endian TEXT DEFAULT 'ABCD';
ALTER TABLE metrics ADD COLUMN IF NOT EXISTS rw TEXT DEFAULT 'R';

-- 3 台光储核心设备（kind='pv_ess'，不参与配电 feeder 电费口径）
INSERT INTO devices(id,name,kind,driver,meta) VALUES
  ('pv-inv-01','1# 光伏并网逆变器 SG110CX','pv_ess','rtu','{"slave":17,"bus":"RS485-Bus2","baud":"9600,8,N,1","vlan":"VLAN20 (192.168.20.11)","role":"光伏发电逆变"}'),
  ('ess-bms-01','1# 储能锂电簇 BMS','pv_ess','tcp','{"slave":1,"ip":"192.168.20.21:502","vlan":"VLAN20","role":"电芯SOC/SOH与温差监控"}'),
  ('ess-pcs-01','1# 储能变流器 PCS-100kW','pv_ess','tcp','{"slave":2,"ip":"192.168.20.22:502","vlan":"VLAN20","role":"双向变流削峰填谷"}')
ON CONFLICT (id) DO NOTHING;

INSERT INTO metrics(device_id,metric,unit,scale,reg_addr,reg_len,fc,dtype,endian,rw) VALUES
  ('pv-inv-01','dc_u','V',0.1,0x3000,2,'04H','UInt16','ABCD','R'),
  ('pv-inv-01','p','kW',0.1,0x3004,4,'04H','UInt32','ABCD','R'),
  ('pv-inv-01','temp','C',0.1,0x301A,2,'04H','UInt16','ABCD','R'),
  ('pv-inv-01','epi','kWh',0.1,0x3020,4,'04H','UInt32','ABCD','R'),
  ('ess-bms-01','soc','%',0.1,0x0100,2,'03H','UInt16','ABCD','R'),
  ('ess-bms-01','soh','%',0.1,0x0101,2,'03H','UInt16','ABCD','R'),
  ('ess-bms-01','cell_dv','V',0.001,0x0108,2,'03H','UInt16','ABCD','R'),
  ('ess-bms-01','temp','C',0.1,0x010A,2,'03H','UInt16','ABCD','R'),
  ('ess-pcs-01','p','kW',0.1,0x0200,2,'03H','Int16','ABCD','R'),
  ('ess-pcs-01','p_set','kW',0.1,0x0210,2,'06H','Int16','ABCD','RW'),
  ('ess-pcs-01','u','V',0.1,0x0202,2,'03H','UInt16','ABCD','R'),
  ('ess-pcs-01','f','Hz',0.01,0x0206,2,'03H','UInt16','ABCD','R')
ON CONFLICT (device_id,metric) DO NOTHING;

-- 1) 采集通信与断线排障记录
CREATE TABLE IF NOT EXISTS comm_logs(
  id SERIAL PRIMARY KEY,
  ts TIMESTAMPTZ NOT NULL DEFAULT now(),
  device_id TEXT, proto TEXT, slave_addr TEXT, func_code TEXT, reg_addr TEXT,
  tx_hex TEXT, rx_hex TEXT, latency_ms INT,
  status TEXT, fault_stage TEXT, handling_note TEXT
);

-- 2) 光伏/储能标准化巡检表
CREATE TABLE IF NOT EXISTS inspections(
  id SERIAL PRIMARY KEY,
  code TEXT, area TEXT, inspector TEXT, shift TEXT,
  check_date DATE DEFAULT CURRENT_DATE,
  items JSONB NOT NULL DEFAULT '[]',
  abnormal_count INT DEFAULT 0,
  erp_sync_status TEXT DEFAULT 'synced',
  created_at TIMESTAMPTZ DEFAULT now()
);

-- 3) 设备缺陷台账（CMMS/ERP PM 缺陷通知单）
CREATE TABLE IF NOT EXISTS defects(
  id SERIAL PRIMARY KEY,
  code TEXT, device_id TEXT, level TEXT, category TEXT,
  title TEXT, phenomenon TEXT, root_cause TEXT, action_taken TEXT,
  reporter TEXT, handler TEXT, wo_id INT, erp_notif_no TEXT,
  status TEXT DEFAULT 'open',
  found_at TIMESTAMPTZ DEFAULT now(), resolved_at TIMESTAMPTZ
);

-- 4) 电气倒闸操作票（五防 / 唱票 / 监护 / LOTO）
CREATE TABLE IF NOT EXISTS switching_tickets(
  id SERIAL PRIMARY KEY,
  ticket_no TEXT, title TEXT, task_mode TEXT, target_device TEXT,
  operator TEXT, guardian TEXT, duty_leader TEXT,
  status TEXT DEFAULT 'completed',
  steps JSONB NOT NULL DEFAULT '[]',
  safety_checks JSONB NOT NULL DEFAULT '[]',
  issued_at TIMESTAMPTZ DEFAULT now(), finished_at TIMESTAMPTZ
);

-- ===== 种子数据（仅空表时插入，用户数据不复活）=====
INSERT INTO comm_logs(ts,device_id,proto,slave_addr,func_code,reg_addr,tx_hex,rx_hex,latency_ms,status,fault_stage,handling_note)
SELECT * FROM (VALUES
  (now() - interval '9 minutes','meter-main','Modbus-RTU','0x01','03H','0x2000','01 03 20 00 00 02 CF CB','01 03 04 00 00 0E E6 7F D9',18,'ok','轮询正常','03H 读保持寄存器；原始 0x0EE6=3814 ×0.1=381.4V，CRC16 校验通过'),
  (now() - interval '8 minutes','pv-inv-01','Modbus-RTU','0x11','04H','0x3000','11 04 30 00 00 01 3C 5A','11 04 02 1C 0A F0 34',22,'ok','轮询正常','04H 读输入寄存器；直流母线 720.5V，网关 RTU→TCP 透明转发'),
  (now() - interval '7 minutes','ess-pcs-01','Modbus-TCP','0x02','06H','0x0210','00 01 00 00 00 06 02 06 02 10 FC E0','00 01 00 00 00 06 02 06 02 10 FC E0',12,'ok','遥调成功','谷段写 p_set=-800 → -80.0kW 充电；MBAP 头无 CRC，功能码 06H 写单寄存器'),
  (now() - interval '6 minutes','meter-main','Modbus-RTU','0x01','03H','0x2000','01 03 20 00 00 02 CF CB','(无响应)',520,'timeout','Retry 1/3','Rx Timeout >500ms，检查 RS485 A+/B- 极性'),
  (now() - interval '5 minutes','meter-main','Modbus-RTU','0x01','03H','0x2000','01 03 20 00 00 02 CF CB','(无响应)',520,'timeout','Retry 3/3','连续 3 次超时；15s 通信看门狗触发 alarm.comm(CRITICAL)，冻结最后有效值并置灰 OFFLINE'),
  (now() - interval '4 minutes','meter-main','Modbus-RTU','0x01','03H','0x2000','01 03 20 00 00 02 CF CB','01 03 04 00 00 0E E2 7E 1A',19,'ok','Auto-Heal 自动恢复','现场排查：3# 表箱 RS485 B- 端子松动，紧固并补接 120Ω 终端电阻；收到合法 RX 帧自动消警')
) AS s(ts,device_id,proto,slave_addr,func_code,reg_addr,tx_hex,rx_hex,latency_ms,status,fault_stage,handling_note)
WHERE NOT EXISTS (SELECT 1 FROM comm_logs);

INSERT INTO inspections(code,area,inspector,shift,check_date,items,abnormal_count,erp_sync_status)
SELECT * FROM (VALUES
  ('XJ-202611-001','光伏阵列与逆变器区','王工','白班',CURRENT_DATE,
   '[{"k":"组件热斑","v":"无","ok":true},{"k":"MC4 接头","v":"紧固无发黑","ok":true},{"k":"直流浪涌保护器","v":"窗口绿色","ok":true},{"k":"逆变器风扇防尘网","v":"已清扫","ok":true},{"k":"IGBT 温度","v":"48.2C","ok":true},{"k":"并网有功","v":"62.3kW","ok":true}]'::jsonb,0,'synced'),
  ('XJ-202611-002','储能集装箱 BMS/PCS','李工','白班',CURRENT_DATE,
   '[{"k":"簇间压差","v":"16mV (<25mV)","ok":true},{"k":"液冷管路","v":"无凝露无渗漏","ok":true},{"k":"七氟丙烷压力","v":"绿区","ok":true},{"k":"PCS 绝缘阻抗","v":"2.4MΩ","ok":true},{"k":"SOC","v":"78.5%","ok":true},{"k":"消防告警回路","v":"自检正常","ok":false}]'::jsonb,1,'synced'),
  ('XJ-202611-003','0.4kV 低压配电室','张工','夜班',CURRENT_DATE,
   '[{"k":"母排红外测温","v":"42.5C (<65C)","ok":true},{"k":"电容补偿柜","v":"投切正常","ok":true},{"k":"ADL400 通信灯","v":"闪烁正常","ok":true},{"k":"柜体接地","v":"完好","ok":true},{"k":"开关机械指示","v":"与实际一致","ok":true}]'::jsonb,0,'synced')
) AS s(code,area,inspector,shift,check_date,items,abnormal_count,erp_sync_status)
WHERE NOT EXISTS (SELECT 1 FROM inspections);

INSERT INTO defects(code,device_id,level,category,title,phenomenon,root_cause,action_taken,reporter,handler,wo_id,erp_notif_no,status,found_at)
SELECT * FROM (VALUES
  ('QF-202611-001','pv-inv-01','major','接头过热','1# 光伏逆变器 PV3 支路 MC4 端子发热 78℃','红外测温 78℃，高于同工况支路 30℃','MC4 插头压接不实、接触电阻增大','停电重新压接并更换 MC4 端子，复测 41℃','王工','李工',NULL::int,'ERP-PM-20261115-01','processing',now()-interval '2 days'),
  ('QF-202611-002','ess-bms-01','critical','电芯一致性','储能 BMS #3 簇第 14 节电芯压差偏大 32mV','静态压差 32mV > 25mV 阈值','该节电芯容量衰减、内阻偏大','隔离该簇、均衡充放电，计划更换电芯','李工','张工',NULL::int,'ERP-PM-20261115-02','open',now()-interval '6 hours'),
  ('QF-202611-003','feeder-im','major','过负荷','2# 注塑机回路峰值负荷触及 62kW 过载红线','合模瞬间功率 62kW，接近额定 90%','生产排产集中、未错峰','调整生产节拍并下发错峰提醒','张工','王工',NULL::int,'ERP-PM-20261114-03','closed',now()-interval '3 days'),
  ('QF-202611-004','meter-main','minor','通信质量','进线总表 RS485 屏蔽层单端接地整改','偶发 CRC 误码、轮询重试','屏蔽层两端接地形成环流','改为控制室单端接地，补 120Ω 终端电阻','张工','张工',NULL::int,'ERP-PM-20261113-04','processing',now()-interval '4 days')
) AS s(code,device_id,level,category,title,phenomenon,root_cause,action_taken,reporter,handler,wo_id,erp_notif_no,status,found_at)
WHERE NOT EXISTS (SELECT 1 FROM defects);

INSERT INTO switching_tickets(ticket_no,title,task_mode,target_device,operator,guardian,duty_leader,status,steps,safety_checks,issued_at,finished_at)
SELECT * FROM (VALUES
  ('DZ-202611-001','2# 注塑机回路（QF2）由运行转冷备用及挂牌检修操作票','停电检修','feeder-im','王工','李工','张值长','completed',
   '[{"n":1,"txt":"核对模拟图与设备名称编号，确认 QF2 双重编号无误","done":true},{"n":2,"txt":"下发遥控分闸指令断开 QF2 断路器","done":true},{"n":3,"txt":"检查 QF2 三相电流归零、刀闸机械指示在“分”位","done":true},{"n":4,"txt":"拉开 QF2 负荷侧隔离开关","done":true},{"n":5,"txt":"在 QF2 负荷侧验电确无电压","done":true},{"n":6,"txt":"装设 #02 号临时接地线","done":true},{"n":7,"txt":"悬挂“禁止合闸，有人工作！”标示牌 LOTO 并锁定","done":true}]'::jsonb,
   '[{"k":"防止误分误合断路器","ok":true},{"k":"防止带负荷分合隔离开关","ok":true},{"k":"防止带电挂接地线","ok":true},{"k":"防止带接地线合断路器","ok":true},{"k":"防止误入带电间隔","ok":true}]'::jsonb,
   now()-interval '1 day',now()-interval '1 day'+interval '35 minutes'),
  ('DZ-202611-002','1# 储能系统 PCS-01 并网转孤岛检修及 BMS 簇高压箱隔离操作票','孤岛检修','ess-pcs-01','李工','王工','张值长','in_progress',
   '[{"n":1,"txt":"核对 PCS-01 与 BMS #3 簇设备双重编号及当前运行方式","done":true},{"n":2,"txt":"将 PCS 有功设定降至 0kW，确认无充放电电流","done":true},{"n":3,"txt":"断开 PCS 交流并网开关，检查机械分位","done":true},{"n":4,"txt":"断开直流侧高压隔离开关并验电、放电","done":false},{"n":5,"txt":"合上接地开关，装设临时接地线","done":false},{"n":6,"txt":"悬挂“禁止合闸，有人工作！”标示牌 LOTO 并锁定","done":false}]'::jsonb,
   '[{"k":"防止误分误合断路器","ok":true},{"k":"防止带负荷分合隔离开关","ok":true},{"k":"防止带电挂接地线","ok":true},{"k":"防止带接地线合断路器","ok":true},{"k":"防止误入带电间隔","ok":true},{"k":"直流侧验电并充分放电","ok":true}]'::jsonb,
   now()-interval '3 hours',NULL::timestamptz)
) AS s(ticket_no,title,task_mode,target_device,operator,guardian,duty_leader,status,steps,safety_checks,issued_at,finished_at)
WHERE NOT EXISTS (SELECT 1 FROM switching_tickets);
