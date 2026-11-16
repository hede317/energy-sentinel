-- M1 真机接入日：feeder-im + ws-pump 切换到 OpenPLC 沙盘（Modbus TCP 127.0.0.1:5020 slave 1）
-- 寄存器映射见 plc/energy_workshop_plc.st 头注释（HR = %QW 直映）
UPDATE devices SET driver='rtu', meta = meta || '{"host":"127.0.0.1","port":5020,"slave":1,"plc":"OpenPLC v3 sandbox (docker energy-openplc)"}'::jsonb
 WHERE id IN ('feeder-im','ws-pump');

-- feeder-im 点表 → PLC 寄存器（原 ADL400 预留地址作废）
UPDATE metrics SET reg_addr=10, reg_len=1, fc='03H', dtype='UInt16' WHERE device_id='feeder-im' AND metric='u';
UPDATE metrics SET reg_addr=11, reg_len=1, fc='03H', dtype='UInt16' WHERE device_id='feeder-im' AND metric='i';
UPDATE metrics SET reg_addr=12, reg_len=1, fc='03H', dtype='UInt16' WHERE device_id='feeder-im' AND metric='p';
UPDATE metrics SET reg_addr=13, reg_len=1, fc='03H', dtype='UInt16' WHERE device_id='feeder-im' AND metric='pf';
UPDATE metrics SET reg_addr=15, reg_len=2, fc='03H', dtype='UInt32', scale=0.01 WHERE device_id='feeder-im' AND metric='epi';
INSERT INTO metrics(device_id,metric,unit,scale,reg_addr,reg_len,fc,dtype,endian,rw)
  VALUES('feeder-im','f','Hz',0.01,14,1,'03H','UInt16','ABCD','R')
  ON CONFLICT (device_id,metric) DO UPDATE SET reg_addr=14, reg_len=1, fc='03H', dtype='UInt16';

-- ws-pump 点表 → PLC 寄存器（原 NULL 地址激活）
UPDATE metrics SET reg_addr=50, reg_len=1, fc='03H', dtype='UInt16', scale=0.001 WHERE device_id='ws-pump' AND metric='pressure';
UPDATE metrics SET reg_addr=51, reg_len=1, fc='03H', dtype='UInt16', scale=0.1  WHERE device_id='ws-pump' AND metric='flow';
UPDATE metrics SET reg_addr=52, reg_len=1, fc='03H', dtype='UInt16', scale=0.1  WHERE device_id='ws-pump' AND metric='p';
UPDATE metrics SET reg_addr=53, reg_len=1, fc='03H', dtype='UInt16', scale=0.1  WHERE device_id='ws-pump' AND metric='f';
INSERT INTO metrics(device_id,metric,unit,scale,reg_addr,reg_len,fc,dtype,endian,rw)
  VALUES('ws-pump','i','A',0.01,54,1,'03H','UInt16','ABCD','R')
  ON CONFLICT (device_id,metric) DO UPDATE SET reg_addr=54, reg_len=1, fc='03H', dtype='UInt16';

SELECT id, driver, meta->>'host' AS host, meta->>'port' AS port FROM devices WHERE id IN ('feeder-im','ws-pump');
