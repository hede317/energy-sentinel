(* ==================================================================
   EnergySentinel 车间配电沙盘 PLC 程序 v1.2（M1：feeder-im + ws-pump）
   目标运行时：OpenPLC v3（Docker），Modbus TCP 从站 :502

   matiec 语法三坑（本版本已规避）：
   ① 直址变量只允许 CONFIGURATION.VAR_GLOBAL 声明，程序体 VAR_EXTERNAL 引用
   ② 位运算 AND/OR/SHR 只作用于 ANY_BIT（WORD/DWORD），UINT 是数值类型不可位运算
      → 寄存器全部声明为 WORD，数值计算用 WORD_TO_UINT/UINT_TO_WORD 转换
   ③ %MW 在 Modbus 侧偏移 +1024（core/modbus.cpp MIN_16B_RANGE）；
      %QW 与保持寄存器 1:1 直映（int_output → mb_holding_regs），故全用 %QW

   寄存器表（HR = %QW，与 EnergySentinel metrics.reg_addr 一致）：
     HR10 im_u ×10 V    HR11 im_i ×100 A   HR12 im_p ×10 kW   HR13 im_pf ×1000
     HR14 im_f ×100     HR15..16 im_epi ×0.01 kWh（u32 高字在前）
     HR17 im 状态 bit0=run bit1=trip bit2=过载演练中
     HR50 pump 压力 ×1000 MPa   HR51 flow ×10   HR52 p ×10 kW
     HR53 变频频率 ×10 Hz       HR54 泵电流 ×100 A   HR55 状态 bit0=运行
     HR56 pump SP 命令 ×1000（RW，主站写）  HR57 demand 命令 ×100 %（RW）
     HR60 im 命令 bit0=过载演练(30s) bit2=分闸 bit3=复位（边沿触发，PLC 自动清位）
   TASK：T#100ms → dt = 0.1s；噪声 = 多频正弦叠加（确定性的"伪随机"）
   ================================================================== *)

FUNCTION scale_u : UINT
  VAR_INPUT x : REAL; END_VAR
  IF x < 0.0 THEN
    scale_u := 0;
  ELSIF x > 65535.0 THEN
    scale_u := 65535;
  ELSE
    scale_u := REAL_TO_UINT(x);
  END_IF;
END_FUNCTION

PROGRAM prog0
  VAR_EXTERNAL
    q10 : WORD;
    q11 : WORD;
    q12 : WORD;
    q13 : WORD;
    q14 : WORD;
    q15 : WORD;
    q16 : WORD;
    q17 : WORD;
    q50 : WORD;
    q51 : WORD;
    q52 : WORD;
    q53 : WORD;
    q54 : WORD;
    q55 : WORD;
    q56 : WORD;
    q57 : WORD;
    q60 : WORD;
  END_VAR
  VAR
    (* ---- 扫描时基 ---- *)
    t_ds : UDINT;      (* 0.1s tick 计数 *)
    t_s  : REAL;       (* 秒 *)
    (* ---- 三角叠加噪声（两路独立相位） ---- *)
    n1 : REAL;
    n2 : REAL;
    (* ---- feeder-im：2# 注塑机回路 ---- *)
    im_u  : REAL;
    im_i  : REAL;
    im_p  : REAL;
    im_pf : REAL;
    im_f  : REAL;
    im_p_base : REAL;
    im_run  : BOOL;
    im_trip : BOOL;     (* 过载跳闸闩锁：须 HR60 bit3 复位 *)
    im_over_t : INT;    (* 过载演练剩余 tick *)
    (* ---- ws-pump：恒压供水 PI 闭环 ---- *)
    sp_m   : REAL;
    pv_m   : REAL;
    freq   : REAL;
    integ  : REAL;
    demand_pct : REAL;
    demand_m3h : REAL;
    pu_power : REAL;
    pu_flow  : REAL;
    pu_i     : REAL;
    pu_target : REAL;
    err_m     : REAL;
    f50       : REAL;
    epi_im_ct : UDINT;
    epi_pu_ct : UDINT;
  END_VAR
  VAR RETAIN
    epi_im_f : REAL;   (* kWh 累计 *)
    epi_pu_f : REAL;
  END_VAR

  (* ---------- 时基 + 噪声 ---------- *)
  t_ds := t_ds + 1;
  IF t_ds > 100000000 THEN t_ds := 0; END_IF;
  t_s := UDINT_TO_REAL(t_ds) / 10.0;
  n1 := SIN(t_s * 13.7) * 0.6 + SIN(t_s * 3.1 + 1.0) * 0.4;
  n2 := SIN(t_s * 8.3) * 0.5 + SIN(t_s * 27.9 + 2.0) * 0.5;

  (* ---------- 命令字 HR60（边沿触发，PLC 自动清位） ---------- *)
  IF WORD_TO_UINT(q60 AND WORD#16#1) > 0 THEN       (* bit0 过载演练 30s *)
    im_over_t := 300;
    q60 := q60 AND WORD#16#FFFE;
  END_IF;
  IF WORD_TO_UINT(q60 AND WORD#16#4) > 0 THEN       (* bit2 遥控分闸 *)
    im_run := FALSE;
    q60 := q60 AND WORD#16#FFFB;
  END_IF;
  IF WORD_TO_UINT(q60 AND WORD#16#8) > 0 THEN       (* bit3 跳闸复位 *)
    im_trip := FALSE;
    q60 := q60 AND WORD#16#FFF7;
  END_IF;

  (* ---------- feeder-im：负载曲线 + 过载保护 ---------- *)
  IF (t_ds MOD 600) < 200 THEN     (* 合模+射胶 55kW 20s *)
    im_p_base := 55.0;
  ELSIF (t_ds MOD 600) < 350 THEN  (* 保压 30kW 15s *)
    im_p_base := 30.0;
  ELSE                             (* 待机 6kW 25s *)
    im_p_base := 6.0;
  END_IF;
  IF im_over_t > 0 THEN
    im_p_base := im_p_base + 40.0;
    im_over_t := im_over_t - 1;
  END_IF;

  IF im_trip THEN
    im_run := FALSE;
  ELSE
    im_run := TRUE;
  END_IF;
  IF im_run THEN
    im_p := im_p_base;
  ELSE
    im_p := 0.0;
  END_IF;
  IF im_run AND (im_p > 81.0) THEN (* 90% 额定过流保护，跳闸闩锁 *)
    im_trip := TRUE;
    im_run := FALSE;
    im_p := 0.0;
  END_IF;

  im_u := 380.0 + 2.0 * SIN(t_s / 90.0 * 3.14159) + n1 * 0.01;
  im_pf := 0.78 + n2 * 0.0003;
  IF im_p > 0.1 THEN
    im_i := im_p * 1000.0 / (1.732 * im_u * im_pf);
  ELSE
    im_i := 0.0;
  END_IF;
  im_f := 50.0 + 0.02 * SIN(t_s / 37.0 * 3.14159);

  epi_im_f := epi_im_f + im_p * 0.1 / 3600.0;
  epi_im_ct := REAL_TO_UDINT(epi_im_f * 100.0);

  q10 := UINT_TO_WORD(scale_u(im_u * 10.0));
  q11 := UINT_TO_WORD(scale_u(im_i * 100.0));
  q12 := UINT_TO_WORD(scale_u(im_p * 10.0));
  q13 := UINT_TO_WORD(scale_u(im_pf * 1000.0));
  q14 := UINT_TO_WORD(scale_u(im_f * 100.0));
  q15 := UDINT_TO_WORD(epi_im_ct / 65536);
  q16 := UDINT_TO_WORD(epi_im_ct MOD 65536);
  q17 := WORD#0;
  IF im_run THEN q17 := q17 OR WORD#16#1; END_IF;
  IF im_trip THEN q17 := q17 OR WORD#16#2; END_IF;
  IF im_over_t > 0 THEN q17 := q17 OR WORD#16#4; END_IF;

  (* ---------- ws-pump：恒压供水 PI 闭环 ---------- *)
  sp_m := WORD_TO_REAL(q56) / 1000.0;
  IF sp_m < 0.05 THEN sp_m := 0.42; END_IF;
  demand_pct := WORD_TO_REAL(q57) / 100.0;
  demand_m3h := 40.0 * (1.0 + demand_pct) + n2 * 0.05;

  f50 := freq / 50.0;
  pu_target := f50 * 0.75 - demand_m3h * 0.004;
  pv_m := pv_m + (pu_target - pv_m) * 0.12;
  err_m := sp_m - pv_m;
  integ := integ + err_m * 0.1 * 0.15;
  IF integ > 30.0 THEN integ := 30.0; END_IF;
  IF integ < -30.0 THEN integ := -30.0; END_IF;
  freq := 50.0 * (0.5 + 0.8 * err_m + integ);
  IF freq > 50.0 THEN freq := 50.0; END_IF;
  IF freq < 20.0 THEN freq := 20.0; END_IF;

  f50 := freq / 50.0;
  pu_power := 7.5 * f50 * f50 * f50 + n1 * 0.02;
  pu_flow := 0.9 * demand_m3h + f50 * 8.0;
  pu_i := pu_power * 1000.0 / (1.732 * 380.0 * 0.99);

  epi_pu_f := epi_pu_f + pu_power * 0.1 / 3600.0;
  epi_pu_ct := REAL_TO_UDINT(epi_pu_f * 100.0);

  q50 := UINT_TO_WORD(scale_u(pv_m * 1000.0));
  q51 := UINT_TO_WORD(scale_u(pu_flow * 10.0));
  q52 := UINT_TO_WORD(scale_u(pu_power * 10.0));
  q53 := UINT_TO_WORD(scale_u(freq * 10.0));
  q54 := UINT_TO_WORD(scale_u(pu_i * 100.0));
  q55 := WORD#1;
END_PROGRAM

CONFIGURATION Config0
  VAR_GLOBAL
    q10 AT %QW10 : WORD;
    q11 AT %QW11 : WORD;
    q12 AT %QW12 : WORD;
    q13 AT %QW13 : WORD;
    q14 AT %QW14 : WORD;
    q15 AT %QW15 : WORD;
    q16 AT %QW16 : WORD;
    q17 AT %QW17 : WORD;
    q50 AT %QW50 : WORD;
    q51 AT %QW51 : WORD;
    q52 AT %QW52 : WORD;
    q53 AT %QW53 : WORD;
    q54 AT %QW54 : WORD;
    q55 AT %QW55 : WORD;
    q56 AT %QW56 : WORD;
    q57 AT %QW57 : WORD;
    q60 AT %QW60 : WORD;
  END_VAR
  RESOURCE Res0 ON PLC
    TASK Main(INTERVAL := T#100ms, PRIORITY := 0);
    PROGRAM Inst0 WITH Main : prog0;
  END_RESOURCE
END_CONFIGURATION
