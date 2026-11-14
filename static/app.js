/* EnergySentinel 前端 —— 单文件，无构建链 */
'use strict';

const CFG = {
  'meter-main': { color: '#f43f5e', order: 0 },
  'feeder-ac':  { x: 140, color: '#f59e0b', icon: 'ac', rated: 75, order: 1 },
  'feeder-im':  { x: 353, color: '#06b6d4', icon: 'im', rated: 90, order: 2 },
  'feeder-lt':  { x: 566, color: '#84cc16', icon: 'lt', rated: 50, order: 3 },
  'feeder-it':  { x: 780, color: '#a78bfa', icon: 'it', rated: 65, order: 4 },
  'ws-pump':    { color: '#38bdf8', order: 5, water: true },
};
// 配电一次接线图只画 4 条分回路；供水实验台是独立工艺场景，没有 x/icon，混进来会崩
const FEEDERS = Object.keys(CFG).filter(k => k !== 'meter-main' && !CFG[k].water);
const TREND_DEVS = ['meter-main', ...FEEDERS, 'ws-pump'];

/* 精细化工业负载图元（画笔中心 x, y；统一用 .ico 线框样式） */
const ICONS = {
  // 空压机：卧式储气罐 + 支腿 + 电机风扇叶轮 + 排气立管
  ac: (x, y) => `
    <rect x="${x - 40}" y="${y - 8}" width="58" height="26" rx="12" class="ico-fill"/>
    <path d="M${x - 30} ${y - 8} V${y + 18} M${x + 8} ${y - 8} V${y + 18}" class="ico"/>
    <path d="M${x - 26} ${y + 18} V${y + 27} M${x + 4} ${y + 18} V${y + 27}" class="ico"/>
    <rect x="${x + 18}" y="${y - 17}" width="22" height="22" rx="3" class="ico-fill"/>
    <circle cx="${x + 29}" cy="${y - 6}" r="8" class="ico"/>
    <path d="M${x + 29} ${y - 14} V${y + 2} M${x + 21} ${y - 6} H${x + 37} M${x + 23} ${y - 12} L${x + 35} ${y} M${x + 35} ${y - 12} L${x + 23} ${y}" class="ico-accent"/>
    <path d="M${x - 6} ${y - 8} V${y - 24} H${x + 14}" class="ico"/>
    <path d="M${x + 14} ${y - 28} L${x + 20} ${y - 24} L${x + 14} ${y - 20} Z" class="ico-fill"/>`,
  // 注塑机：料斗 + 螺杆机筒 + 合模机构 + 机架
  im: (x, y) => `
    <path d="M${x - 30} ${y - 24} L${x - 16} ${y - 24} L${x - 20} ${y - 12} L${x - 26} ${y - 12} Z" class="ico-fill"/>
    <rect x="${x - 24}" y="${y - 12}" width="48" height="16" rx="3" class="ico-fill"/>
    <path d="M${x - 18} ${y - 4} L${x + 18} ${y - 4} M${x - 18} ${y - 8} L${x + 18} ${y - 8}" class="ico-accent"/>
    <path d="M${x - 11} ${y - 8} l4 4 l-4 4 l4 4" class="ico-accent"/>
    <rect x="${x + 24}" y="${y - 14}" width="12" height="20" rx="2" class="ico-fill"/>
    <path d="M${x - 32} ${y + 4} H${x + 36} M${x - 28} ${y + 4} V${y + 24} M${x + 32} ${y + 4} V${y + 24}" class="ico"/>
    <path d="M${x - 36} ${y + 24} H${x + 40}" class="ico"/>`,
  // 照明与空调：发光灯具 + 光晕 + 风机
  lt: (x, y) => `
    <path d="M${x - 40} ${y - 20} H${x - 6}" class="ico"/>
    <path d="M${x - 36} ${y - 20} L${x - 32} ${y - 8} H${x - 14} L${x - 10} ${y - 20} Z" class="ico-fill"/>
    <path d="M${x - 30} ${y - 4} l-4 8 M${x - 23} ${y - 4} v8 M${x - 16} ${y - 4} l4 8" class="ico-accent"/>
    <circle cx="${x + 24}" cy="${y}" r="15" class="ico"/>
    <circle cx="${x + 24}" cy="${y}" r="3" class="ico-fill"/>
    <path d="M${x + 24} ${y - 12} q9 3 9 12 M${x + 36} ${y} q-3 9 -12 9 M${x + 12} ${y + 12} q-9 -3 -9 -12 M${x + 12} ${y} q3 -9 12 -9" class="ico-accent"/>
    <path d="M${x + 6} ${y + 22} H${x + 42}" class="ico"/>`,
  // IT 机房：双层服务器机柜 + 闪烁指示灯
  it: (x, y) => `
    <rect x="${x - 26}" y="${y - 28}" width="52" height="54" rx="3" class="ico-fill"/>
    <path d="M${x - 26} ${y - 10} H${x + 26} M${x - 26} ${y + 8} H${x + 26}" class="ico"/>
    <path d="M${x - 18} ${y - 22} H${x + 6} M${x - 18} ${y - 4} H${x + 6} M${x - 18} ${y + 14} H${x + 6}" class="ico-accent"/>
    <circle cx="${x + 17}" cy="${y - 19}" r="2.5" class="led"/>
    <circle cx="${x + 17}" cy="${y - 1}" r="2.5" class="led led2"/>
    <circle cx="${x + 17}" cy="${y + 17}" r="2.5" class="led"/>
    <path d="M${x - 30} ${y + 26} V${y + 30} M${x + 30} ${y + 26} V${y + 30}" class="ico"/>`,
};

let I18N = {};
const S = {
  lang: localStorage.getItem('es_lang') || 'zh',
  devices: {}, tou_rate: '', counts: { active: 0, acked: 0 },
  metric: 'p', hours: 1, trendSel: new Set(FEEDERS),
  alarmFilter: 'active', energyView: 'day',
  tariff: {}, currency: 'CNY',
  touSpans: null, hist: [], todayKwh: null, soe: [],
  injections: [], uptime_s: null, db_rows: null, audit: [], energyData: null,
};
const THRESHOLDS = {
  u: [{ v: 400, c: '#EF4444' }, { v: 360, c: '#F59E0B' }],
  pressure: [{ v: 0.38, c: '#F59E0B' }],
  f: [{ v: 50.5, c: '#F59E0B' }, { v: 49.5, c: '#F59E0B' }],
  pf: [{ v: 0.8, c: '#F59E0B' }],
};
const hexA = (hex, a) => {
  const n = parseInt(hex.slice(1), 16);
  return `rgba(${n >> 16 & 255},${n >> 8 & 255},${n & 255},${a})`;
};

/* ---------- i18n ---------- */
function t(key, params) {
  let s = I18N[key] || key;
  if (params) for (const [k, v] of Object.entries(params)) s = s.replaceAll(`{${k}}`, v);
  return s;
}
async function loadLang(l) {
  S.lang = l; localStorage.setItem('es_lang', l);
  I18N = await (await fetch(`/i18n/${l}`)).json();
  document.documentElement.lang = l;
  document.documentElement.dir = l === 'ur' ? 'rtl' : 'ltr';
  document.querySelectorAll('[data-i18n]').forEach(el => { el.textContent = t(el.dataset.i18n); });
  document.querySelectorAll('[data-i18n-ph]').forEach(el => { el.placeholder = t(el.dataset.i18nPh); });
  document.getElementById('langSel').value = l;
  buildFeeders(); buildCards(); buildKpis(); bindTrendDevs(); loadApiDoc();
  renderTou(); updateDiagram(); updateCards(); updateKpis(); loadTab(curTab);
}

/* ---------- API ---------- */
async function api(path, opts = {}) {
  const r = await fetch(path, { headers: { 'Content-Type': 'application/json' }, ...opts });
  if (!r.ok) {
    const msg = (await r.json().catch(() => ({}))).error || ('HTTP ' + r.status);
    toast(msg, 'warn');
    throw new Error(msg);
  }
  return r.json();
}
function toast(msg, cls = '') {
  const d = document.createElement('div');
  d.className = 'toast ' + cls; d.textContent = msg;
  document.getElementById('toasts').appendChild(d);
  setTimeout(() => d.remove(), 5000);
}
const f1 = v => (v == null ? '--' : (+v).toFixed(1));
const f2 = v => (v == null ? '--' : (+v).toFixed(2));
const ftime = ts => new Date(ts).toLocaleTimeString([], { hour12: false });
const esc = v => String(v).replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const devName = (id, name) => I18N['dev.' + id] || esc(name || id);
const fdate = ts => new Date(ts).toLocaleString([], { hour12: false, month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' });
const comm = st => st && st._comm !== false;
const brkOn = st => !st || +st.breaker !== 0;
/* 内联 SVG 的显隐显式控制（不依赖 hidden 属性，避免个别浏览器不生效） */
const svgVis = (id, show) => {
  const el = document.getElementById(id);
  if (!el) return;
  el.removeAttribute('hidden');
  el.style.display = show ? '' : 'none';
};

/* ---------- 接线图：0.4kV 一次系统 ---------- */
function buildFeeders() {
  const g = document.getElementById('g-feeders');
  let svg = '';
  for (const id of FEEDERS) {
    const { x, color, icon } = CFG[id];
    svg += `
    <g id="fg-${id}" class="feeder">
      <!-- 母排引下线 -->
      <line x1="${x}" y1="234" x2="${x}" y2="252" class="wire flow" id="fl-${id}"/>
      <!-- 标准刀闸断路器：上/下静触点 + 合闸刀闸 / 分闸斜刀闸 -->
      <g id="brk-${id}" class="brk" style="cursor:pointer" title="点击遥控分合闸">
        <circle cx="${x}" cy="252" r="3" class="contact"/>
        <circle cx="${x}" cy="286" r="3" class="contact"/>
        <line id="knife-c-${id}" x1="${x}" y1="252" x2="${x}" y2="286" class="knife"/>
        <line id="knife-o-${id}" x1="${x}" y1="252" x2="${x + 17}" y2="270" class="knife open" style="display:none"/>
        <text id="brkt-${id}" x="${x + 24}" y="273" class="svg-sub knife-tx">QF合</text>
        <rect x="${x - 20}" y="246" width="52" height="46" fill="transparent" style="pointer-events:all"/>
      </g>
      <line x1="${x}" y1="286" x2="${x}" y2="298" class="wire flow" id="fl2-${id}"/>
      <!-- 智能电表表箱 -->
      <g id="mb-${id}" style="cursor:pointer">
        <rect x="${x - 78}" y="298" width="156" height="92" rx="7" class="meter-box" id="box-${id}"/>
        <rect x="${x - 78}" y="298" width="156" height="3" rx="1.5" fill="${color}" opacity=".9"/>
        <circle id="dot-${id}" cx="${x - 66}" cy="313" r="4" class="dot"/>
        <text x="${x - 56}" y="317" class="svg-name" id="nm-${id}">${t('dev.' + id)}</text>
        <text x="${x + 70}" y="317" text-anchor="end" class="svg-off" id="off-${id}" style="display:none">OFFLINE</text>
        <!-- 内凹液晶数显槽 -->
        <rect x="${x - 70}" y="324" width="140" height="42" rx="5" class="lcd-slot"/>
        <text x="${x}" y="345" text-anchor="middle" class="svg-val num" fill="${color}" id="v-${id}-p">--</text>
        <text x="${x - 66}" y="360" class="lcd-sub">U <tspan class="num" id="v-${id}-u">--</tspan></text>
        <text x="${x - 8}" y="360" class="lcd-sub">I <tspan class="num" id="v-${id}-i">--</tspan></text>
        <text x="${x + 34}" y="360" class="lcd-sub">PF <tspan class="num" id="v-${id}-pf">--</tspan></text>
        <text x="${x + 66}" y="345" text-anchor="end" class="svg-off" id="open-${id}" style="display:none">OPEN</text>
        <text x="${x - 70}" y="382" class="lcd-cap">负载率</text>
        <rect x="${x - 26}" y="375" width="96" height="5" rx="2.5" class="loadtrack"/>
        <rect id="load-${id}" x="${x - 26}" y="375" width="0" height="5" rx="2.5" fill="#10B981"/>
      </g>
      <line x1="${x}" y1="390" x2="${x}" y2="400" class="wire"/>
      <!-- 精细化工业负载图元 -->
      ${ICONS[icon](x, 428)}
      <text x="${x}" y="474" text-anchor="middle" class="svg-load-tx">${t('dev.' + id)}</text>
    </g>`;
  }
  g.innerHTML = svg;
  for (const id of FEEDERS) {
    document.getElementById('mb-' + id).onclick = () => {
      S.trendSel = new Set([id]); switchTab('trend');
    };
    document.getElementById('brk-' + id).onclick = () => {
      openBreakerModal(id, !brkOn(S.devices[id]));
    };
  }
  // QF0 主进线总闸：拉闸后全站失电
  const qf0 = document.getElementById('qf0');
  if (qf0) qf0.onclick = () => openBreakerModal('meter-main', !brkOn(S.devices['meter-main']));
}

function updateDiagram() {
  const d = S.devices;
  const put = (id, k, v) => { const el = document.getElementById(`v-${id}-${k}`); if (el) el.textContent = v; };
  const offline = (id, isOff) => {
    const box = document.getElementById('box-' + id);
    if (box) box.classList.toggle('offline', isOff);
    const fg = document.getElementById('fg-' + id);
    if (fg) fg.classList.toggle('offline', isOff);
    svgVis('off-' + id, isOff);
    const card = document.getElementById('card-' + id);
    if (card) card.classList.toggle('offline', isOff);
    const cbadge = document.getElementById('offc-' + id);
    if (cbadge) cbadge.hidden = !isOff;
  };

  // 进线总表综合监控舱
  const hasMain = !!d['meter-main'];
  const m = d['meter-main'] || {};
  const mOff = hasMain && !comm(d['meter-main']);
  ['p', 'q', 'pf', 'f', 'i', 'load_rate'].forEach(k => {
    put('meter-main', k, k === 'pf' ? f2(m.pf) : f1(m[k]));
  });
  ['ua', 'ub', 'uc', 'imbalance'].forEach(k => put('meter-main', k, f1(m[k])));
  const busU = document.getElementById('v-bus-u');
  if (busU) busU.textContent = f1(m.u);
  const busI = document.getElementById('v-bus-i');
  if (busI) busI.textContent = f1(m.i);
  const qf0 = document.getElementById('qf0Txt');
  const mainOn = brkOn(d['meter-main']);
  if (qf0) {
    qf0.textContent = mainOn ? 'QF0 合 · CLOSED' : 'QF0 分 · OPEN';
    qf0.style.fill = mainOn ? '#34D399' : '#F87171';
  }
  svgVis('qf0KnifeC', mainOn);
  svgVis('qf0KnifeO', !mainOn);
  const mvCap = document.getElementById('mvCap');
  if (mvCap) mvCap.textContent = mainOn ? '250kVA / 400A' : '全站失电';
  const mvDot = document.getElementById('dot-meter-main');
  if (mvDot) mvDot.classList.toggle('bad', mOff);
  const mvBox = document.getElementById('box-meter-main');
  if (mvBox) mvBox.classList.toggle('offline', mOff);
  svgVis('off-meter-main', mOff);
  const lr = Math.max(0, Math.min(100, m.load_rate || 0));
  const mvBar = document.getElementById('mvLoadBar');
  if (mvBar) {
    mvBar.setAttribute('width', (118 * lr / 100).toFixed(1));
    mvBar.setAttribute('fill', lr > 85 ? '#EF4444' : lr > 70 ? '#F59E0B' : '#10B981');
  }
  const nmMain = document.getElementById('nm-meter-main');
  if (nmMain) nmMain.textContent = t('dev.meter-main');

  // 4 条分回路
  FEEDERS.forEach(id => {
    const st = d[id] || {};
    const isOff = !!d[id] && !comm(d[id]);
    put(id, 'p', f1(st.p)); put(id, 'u', f1(st.u));
    put(id, 'i', f1(st.i)); put(id, 'pf', f2(st.pf));
    const dot = document.getElementById('dot-' + id);
    if (dot) dot.classList.toggle('bad', isOff);
    offline(id, isOff);

    const on = brkOn(d[id]);
    svgVis('knife-c-' + id, on);
    svgVis('knife-o-' + id, !on);
    const kt = document.getElementById('brkt-' + id);
    if (kt) { kt.textContent = on ? 'QF合' : 'QF分'; kt.style.fill = on ? '#34D399' : '#F87171'; }
    svgVis('open-' + id, !on);
    const box = document.getElementById('box-' + id);
    if (box) box.classList.toggle('alarm', on && f1(st.p) !== '--' && st.p > CFG[id].rated * 0.9);
    const fg = document.getElementById('fg-' + id);
    if (fg) fg.classList.toggle('open', !on);

    const loadPct = Math.max(0, Math.min(100, (st.p || 0) / CFG[id].rated * 100));
    const bar = document.getElementById('load-' + id);
    if (bar) {
      bar.setAttribute('width', (96 * loadPct / 100).toFixed(1));
      bar.setAttribute('fill', loadPct > 85 ? '#EF4444' : loadPct > 70 ? '#F59E0B' : '#10B981');
    }
    // 分闸/离线时停止电流流动动画
    const spd = !on || isOff || !d[id] ? 'stop' : st.p > 40 ? 'fast' : '';
    ['fl-' + id, 'fl2-' + id].forEach(fid => {
      const el = document.getElementById(fid);
      if (!el) return;
      el.classList.remove('slow', 'fast', 'flow');
      if (spd === 'stop') { el.style.opacity = '.35'; }
      else { el.style.opacity = '1'; el.classList.add('flow'); if (spd) el.classList.add(spd); }
    });
    const nm = document.getElementById('nm-' + id);
    if (nm) nm.textContent = t('dev.' + id);
  });

  // 母排电流动画随总表负载率变速；总闸拉开 = 全站失电
  const bus = document.querySelector('.busbar');
  if (bus) {
    bus.classList.remove('slow', 'fast', 'flow');
    if (!mainOn || mOff) { bus.style.opacity = '.25'; }
    else {
      bus.style.opacity = '1'; bus.classList.add('flow');
      if (lr > 75) bus.classList.add('fast'); else if (lr < 40) bus.classList.add('slow');
    }
  }
}

/* ---------- 设备卡（2列×3行紧凑工业矩阵） ---------- */
function buildCards() {
  const wrap = document.getElementById('devCards');
  wrap.innerHTML = Object.keys(CFG).sort((a, b) => CFG[a].order - CFG[b].order).map(id => {
    const rows = CFG[id].water
      ? `<div class="row"><span>MPa</span><span class="num" id="c-${id}-pressure">--</span></div>
         <div class="row"><span>Hz</span><span class="num" id="c-${id}-f">--</span></div>
         <div class="row"><span>m³/h</span><span class="num" id="c-${id}-flow">--</span></div>
         <div class="row"><span>EPi kWh</span><span class="num" id="c-${id}-epi">--</span></div>`
      : `<div class="row"><span>U</span><span class="num" id="c-${id}-u">--</span></div>
         <div class="row"><span>I</span><span class="num" id="c-${id}-i">--</span></div>
         <div class="row"><span>PF</span><span class="num" id="c-${id}-pf">--</span></div>
         <div class="row"><span>EPi kWh</span><span class="num" id="c-${id}-epi">--</span></div>`;
    return `
    <div class="devcard" id="card-${id}" style="border-top:3px solid ${CFG[id].color}">
      <div class="name"><span>${t('dev.' + id)}</span><span><span class="offbadge" id="offc-${id}" hidden>OFFLINE</span><span class="dot" id="cdot-${id}"></span></span></div>
      <div class="p num"><span id="c-${id}-p">--</span> <small>kW</small></div>
      ${rows}
    </div>`;
  }).join('');
}
function updateCards() {
  for (const id of Object.keys(CFG)) {
    const st = S.devices[id]; if (!st) continue;
    ['p', 'u', 'i', 'epi', 'pressure', 'f', 'flow'].forEach(k => {
      const el = document.getElementById(`c-${id}-${k}`); if (el) el.textContent = f2(st[k]);
    });
    const pf = document.getElementById(`c-${id}-pf`); if (pf) pf.textContent = f2(st.pf);
    const off = !comm(st);
    const dot = document.getElementById(`cdot-${id}`); if (dot) dot.classList.toggle('bad', off);
    const card = document.getElementById('card-' + id);
    if (card) card.classList.toggle('offline', off);
    const badge = document.getElementById('offc-' + id);
    if (badge) badge.hidden = !off;
  }
}

/* ---------- TOU / 铃铛 ---------- */
function renderTou() {
  const b = document.getElementById('touBadge');
  b.textContent = t('tou.now') + ': ' + t('tou.' + (S.tou_rate || 'flat'));
  b.className = 'badge ' + (S.tou_rate || 'flat');
  const bc = document.getElementById('bellCount');
  bc.hidden = !(S.counts.active > 0);
  bc.textContent = S.counts.active;
}

/* ---------- SSE ---------- */
function setSse(ok) {
  const b = document.getElementById('sseBadge');
  if (b) { b.classList.toggle('sse-ok', ok); b.classList.toggle('sse-bad', !ok); }
  const tx = document.getElementById('sseTxt');
  if (tx) tx.textContent = ok ? 'LIVE' : 'RECONNECT';
  const st = document.getElementById('stSse');
  if (st) { st.innerHTML = `<i class="st-dot"></i><b>SSE</b> ${ok ? 'Connected' : 'Reconnecting'}`; }
  const bar = document.getElementById('statusBar');
  if (bar) bar.classList.toggle('offline-sse', !ok);
}
function openSSE() {
  const es = new EventSource('/api/stream');
  es.onopen = () => setSse(true);
  es.onerror = () => setSse(false);
  es.onmessage = e => {
    const m = JSON.parse(e.data);
    if (m.type === 'tick') {
      setSse(true);
      S.devices = m.devices; S.counts = m.alarm_counts || S.counts;
      if (m.tou_rate) S.tou_rate = m.tou_rate;
      if (m.injections) S.injections = m.injections;
      if (m.uptime_s != null) S.uptime_s = m.uptime_s;
      if (m.db_rows != null) S.db_rows = m.db_rows;
      const main = S.devices['meter-main'];
      if (main) {
        S.hist.push(main.p); if (S.hist.length > 120) S.hist.shift();
      }
      renderTou(); updateDiagram(); updateCards(); updateWater();
      updateStatusbar(); updateKpis(); renderSpark();
      renderInjections(); updateStatusBar();
    } else if (m.type === 'alarm') {
      const a = m.alarm || {};
      if (m.event === 'triggered') {
        toast('ALARM · ' + locMsg(a), 'danger');
        soePush(a);
        if (curTab === 'alarms') loadAlarms();
      } else {
        const it = S.soe.find(x => x.id === a.id);
        if (it) { it.status = m.event === 'acked' ? 'acked' : 'closed'; }
        else if (m.event === 'closed') soePush(a);
        renderSoe();
        if (curTab === 'alarms') loadAlarms();
      }
    } else if (m.type === 'audit') {
      loadAudit();
    }
  };
}

/* 激活故障倒计时胶囊 */
function renderInjections() {
  const host = document.getElementById('injectionPills');
  if (!host) return;
  const list = S.injections || [];
  host.innerHTML = list.map(x => {
    const left = x.remain_s < 0 ? '持续' : `${x.remain_s}s`;
    return `<span class="inj-pill"><svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 3l9 17H3L12 3Z"/><path d="M12 9v5"/></svg>${esc(x.label)} · 剩余 <b class="num">${left}</b></span>`;
  }).join('');
}

/* 底部工业状态条 */
function updateStatusBar() {
  const u = document.getElementById('stUptime');
  if (u && S.uptime_s != null) {
    const s = S.uptime_s, h = Math.floor(s / 3600), mm = Math.floor(s % 3600 / 60), ss = s % 60;
    u.textContent = `${h}h ${String(mm).padStart(2, '0')}m ${String(ss).padStart(2, '0')}s`;
  }
  const r = document.getElementById('stRows');
  if (r && S.db_rows != null) r.textContent = S.db_rows.toLocaleString();
  const p = document.getElementById('stPeriod');
  if (p) p.textContent = '2000';
}

/* 操作审计双 Tab */
async function loadAudit() {
  try {
    const d = await api('/api/audit');
    S.audit = d.audit || [];
    const host = document.getElementById('auditList');
    if (host) {
      host.innerHTML = S.audit.length ? S.audit.map(a => `
        <div class="audit-row">
          <span class="when num">${fdate(new Date(a.ts).getTime())}</span>
          <span class="who">${esc(a.user || 'system')}</span>
          <span class="act"><span class="audit-badge ${esc(a.action)}">${esc(a.action)}</span>${esc(a.device || '')} <span class="muted">${esc(a.detail || '')}</span></span>
        </div>`).join('') : '<p class="muted">暂无操作记录</p>';
    }
    const c = document.getElementById('auditTabCount');
    if (c) c.textContent = S.audit.length;
  } catch (e) { /* 忽略 */ }
}
function bindPanelTabs() {
  document.querySelectorAll('.ptab').forEach(btn => {
    btn.onclick = () => {
      document.querySelectorAll('.ptab').forEach(b => b.classList.toggle('active', b === btn));
      const aud = btn.dataset.pane === 'audit';
      document.getElementById('soePane').hidden = aud;
      document.getElementById('auditPane').hidden = !aud;
      if (aud) loadAudit();
    };
  });
}
function locMsg(a) {
  const meta = a.meta || {};
  if (meta.key && I18N[meta.key]) return t(meta.key, meta.params);
  return a.message || '';
}

/* ---------- Tabs ---------- */
let curTab = 'overview';
function switchTab(name) {
  curTab = name;
  document.querySelectorAll('nav#tabs button').forEach(b => b.classList.toggle('active', b.dataset.tab === name));
  document.querySelectorAll('.tab').forEach(s => s.classList.toggle('active', s.id === 'tab-' + name));
  loadTab(name);
}
function loadTab(name) {
  ({
    trend: loadTrend, water: loadWaterTrend, energy: loadEnergy, alarms: loadAlarms,
    workorders: loadWos, devices: () => { loadDevices(); loadRules(); }, api: loadSettings,
  }[name] || (() => {}))();
}

/* ---------- 趋势（含进线总表） ---------- */
function bindTrendDevs() {
  document.getElementById('trendDevs').innerHTML = TREND_DEVS.map(id =>
    `<label><input type="checkbox" value="${id}" ${S.trendSel.has(id) ? 'checked' : ''}> ${t('dev.' + id)}</label>`).join('');
  document.querySelectorAll('#trendDevs input').forEach(cb => cb.onchange = () => {
    cb.checked ? S.trendSel.add(cb.value) : S.trendSel.delete(cb.value);
    loadTrend();
  });
}
async function loadTrend() {
  const devs = [...S.trendSel]; if (!devs.length) return;
  const d = await api(`/api/history?devices=${devs.join(',')}&metric=${S.metric}&hours=${S.hours}`);
  const series = devs.map(id => ({ name: t('dev.' + id), color: CFG[id].color, data: d.series[id] || [] }));
  drawChart(document.getElementById('trendCanvas'), series, { thresholds: THRESHOLDS[S.metric] || [], bands: S.metric === 'p' });
  document.getElementById('trendLegend').innerHTML = series.map(s =>
    `<span><span class="key" style="background:${s.color}"></span>${s.name}</span>`).join('');
}

/* 通用折线图：支持双 Y 轴 + 鼠标十字准星毛玻璃 Tooltip */
function drawChart(canvas, series, opts = {}) {
  const dpr = window.devicePixelRatio || 1;
  if (!canvas.dataset.h) canvas.dataset.h = canvas.getAttribute('height');
  const W = canvas.clientWidth || 900, H = +canvas.dataset.h;
  const all = series.flatMap(s => s.data);
  if (!all.length) {
    canvas.width = W * dpr; canvas.height = H * dpr;
    const c0 = canvas.getContext('2d'); c0.scale(dpr, dpr); c0.clearRect(0, 0, W, H);
    c0.fillStyle = '#94a3b8'; c0.font = '12px sans-serif';
    c0.fillText(t('trend.nodata'), W / 2 - 30, H / 2);
    canvas._chartState = null;
    return;
  }
  const xs = all.map(p => p[0]);
  const x0 = Math.min(...xs), x1 = Math.max(...xs) || x0 + 1;
  const left = series.filter(s => s.axis !== 'right').flatMap(s => s.data);
  const right = series.filter(s => s.axis === 'right').flatMap(s => s.data);
  const rng = arr => {
    if (!arr.length) return [0, 1];
    let a = Math.min(...arr.map(p => p[1])), b = Math.max(...arr.map(p => p[1]));
    const pad = (b - a) * 0.12 || Math.max(Math.abs(a) * 0.1, 0.1);
    return [a - pad, b + pad];
  };
  const state = {
    W, H, dpr, x0, x1, series, opts,
    yl: rng(left.length ? left : all), yr: rng(right), hasR: right.length > 0,
    cursor: null,
  };
  canvas._chartState = state;
  paintChart(canvas);
  bindChartInteraction(canvas, 'line');
}

function paintChart(canvas) {
  const st = canvas._chartState; if (!st) return;
  const { W, H, dpr, x0, x1, series, opts, yl, yr, hasR } = st;
  const [yl0, yl1] = yl, [yr0, yr1] = yr;
  canvas.width = W * dpr; canvas.height = H * dpr;
  const ctx = canvas.getContext('2d'); ctx.scale(dpr, dpr);
  ctx.clearRect(0, 0, W, H);
  const all = series.flatMap(s => s.data);
  const L = 56, R = hasR ? 54 : 12, T = 12, B = 26;
  const px = v => L + (v - x0) / (x1 - x0) * (W - L - R);
  const mk = (a, b) => v => H - B - (v - a) / (b - a) * (H - T - B);
  const pyL = mk(yl0, yl1), pyR = mk(yr0, yr1);
  const py = (v, s) => s.axis === 'right' ? pyR(v) : pyL(v);
  st.px = px; st.py = py; st.pyL = pyL; st.L = L; st.R = R; st.T = T; st.B = B;
  const fmt = v => v.toFixed(Math.abs(yl1 - yl0) < 5 ? 2 : 0);
  ctx.font = '11px ui-monospace, monospace'; ctx.lineWidth = 1;
  for (let i = 0; i <= 4; i++) {
    const vl = yl0 + (yl1 - yl0) * i / 4, y = pyL(vl);
    ctx.strokeStyle = '#24304d'; ctx.beginPath();
    ctx.moveTo(L, y); ctx.lineTo(W - R, y); ctx.stroke();
    ctx.fillStyle = '#94a3b8'; ctx.fillText(fmt(vl), 6, y + 4);
    if (hasR) {
      const vr = yr0 + (yr1 - yr0) * i / 4;
      ctx.fillStyle = '#64748b'; ctx.fillText(fmt(vr), W - R + 6, y + 4);
    }
  }
  for (let i = 0; i <= 4; i++) {
    const v = x0 + (x1 - x0) * i / 4, x = px(v);
    ctx.fillStyle = '#94a3b8'; ctx.fillText(fdate(v).slice(5), x - 22, H - 8);
  }
  if (opts.bands && S.touSpans) {
    const colors = { peak: 'rgba(245,158,11,.07)', flat: 'rgba(96,165,250,.05)', valley: 'rgba(16,185,129,.06)' };
    let cur = new Date(x0); cur.setMinutes(0, 0, 0);
    let guard = 0;
    while (cur.getTime() < x1 && guard++ < 400) {
      const nx = new Date(cur.getTime() + 3600_000);
      const h = cur.getHours(), rate = touRateOf(h);
      ctx.fillStyle = colors[rate];
      ctx.fillRect(px(cur.getTime()), T, px(Math.min(nx.getTime(), x1)) - px(cur.getTime()), H - T - B);
      cur = nx;
    }
  }
  for (const s of series) {
    if (!s.data.length) continue;
    ctx.beginPath();
    s.data.forEach((p, i) => i ? ctx.lineTo(px(p[0]), py(p[1], s)) : ctx.moveTo(px(p[0]), py(p[1], s)));
    const g = ctx.createLinearGradient(0, T, 0, H - B);
    g.addColorStop(0, hexA(s.color, .22)); g.addColorStop(1, hexA(s.color, 0));
    ctx.save();
    ctx.lineTo(px(s.data[s.data.length - 1][0]), H - B);
    ctx.lineTo(px(s.data[0][0]), H - B);
    ctx.closePath(); ctx.fillStyle = g; ctx.fill();
    ctx.restore();
    ctx.strokeStyle = s.color; ctx.lineWidth = 1.8;
    if (s.axis === 'right') ctx.setLineDash([5, 3]);
    ctx.beginPath();
    s.data.forEach((p, i) => i ? ctx.lineTo(px(p[0]), py(p[1], s)) : ctx.moveTo(px(p[0]), py(p[1], s)));
    ctx.stroke(); ctx.setLineDash([]);
  }
  (opts.thresholds || []).forEach(th => {
    if (th.v < yl0 || th.v > yl1) return;
    const y = pyL(th.v);
    ctx.strokeStyle = th.c; ctx.setLineDash([6, 5]); ctx.lineWidth = 1;
    ctx.beginPath(); ctx.moveTo(L, y); ctx.lineTo(W - R, y); ctx.stroke();
    ctx.setLineDash([]);
    ctx.fillStyle = th.c; ctx.fillText(String(th.v), W - R - 32, y - 3);
  });
  if (st.cursor != null) drawLineCrosshair(ctx, st);
}

/* 十字准星：吸附最近时间点 + 各曲线高亮圆点 */
function drawLineCrosshair(ctx, st) {
  const { cursor, series, px, py, L, R, T, B, H } = st;
  const targetTs = st.x0 + cursor * (st.x1 - st.x0);
  let best = null, bestD = Infinity;
  for (const s of series) for (const p of s.data) {
    const d = Math.abs(p[0] - targetTs);
    if (d < bestD) { bestD = d; best = p[0]; }
  }
  if (best == null) return;
  const x = px(best);
  ctx.save();
  ctx.strokeStyle = 'rgba(34,211,238,.75)'; ctx.setLineDash([5, 4]); ctx.lineWidth = 1;
  ctx.beginPath(); ctx.moveTo(x, T); ctx.lineTo(x, H - B); ctx.stroke();
  ctx.setLineDash([]);
  for (const s of series) {
    if (!s.data.length) continue;
    let nearest = s.data[0];
    for (const p of s.data) if (Math.abs(p[0] - best) < Math.abs(nearest[0] - best)) nearest = p;
    if (Math.abs(nearest[0] - best) > (st.x1 - st.x0) / 30) continue;
    const y = py(nearest[1], s);
    ctx.beginPath(); ctx.arc(x, y, 5, 0, Math.PI * 2);
    ctx.fillStyle = s.color; ctx.fill();
    ctx.strokeStyle = '#fff'; ctx.lineWidth = 2; ctx.stroke();
  }
  ctx.restore();
  st.snapTs = best;
}

/* 共享 Tooltip 浮层 */
let chartTip = null;
function showChartTip(ev, html) {
  if (!chartTip) {
    chartTip = document.createElement('div');
    chartTip.className = 'chart-tip';
    document.body.appendChild(chartTip);
  }
  chartTip.innerHTML = html;
  chartTip.style.display = 'block';
  const pad = 14, w = chartTip.offsetWidth, h = chartTip.offsetHeight;
  let x = ev.clientX + pad, y = ev.clientY + pad;
  if (x + w > window.innerWidth - 8) x = ev.clientX - w - pad;
  if (y + h > window.innerHeight - 8) y = ev.clientY - h - pad;
  chartTip.style.left = x + 'px'; chartTip.style.top = y + 'px';
}
function hideChartTip() { if (chartTip) chartTip.style.display = 'none'; }

function bindChartInteraction(canvas, kind) {
  if (canvas._bound) return;
  canvas._bound = true;
  canvas.onmousemove = ev => {
    const st = canvas._chartState;
    if (!st) return;
    const rect = canvas.getBoundingClientRect();
    if (kind === 'line') {
      const rel = (ev.clientX - rect.left - st.L) / (st.W - st.L - st.R);
      st.cursor = Math.max(0, Math.min(1, rel));
      paintChart(canvas);
      const ts = st.snapTs;
      if (ts == null) return hideChartTip();
      const rows = st.series.map(s => {
        if (!s.data.length) return '';
        let n = s.data[0];
        for (const p of s.data) if (Math.abs(p[0] - ts) < Math.abs(n[0] - ts)) n = p;
        return `<div class="tr"><i style="background:${s.color}"></i><span>${esc(s.name)}</span><b>${n[1].toFixed(2)}${s.axis === 'right' ? ' (右轴)' : ''}</b></div>`;
      }).join('');
      showChartTip(ev, `<div class="tt">${fdate(ts)}</div>${rows}`);
    } else {
      const rel = (ev.clientX - rect.left - st.L) / (st.W - st.L - st.R);
      const n = st.items.length;
      const idx = Math.max(0, Math.min(n - 1, Math.floor(rel * n)));
      paintStackedBar(canvas, idx);
      const it = st.items[idx];
      showChartTip(ev, `<div class="tt">${esc(it.name)}</div>
        <div class="tr"><i style="background:#f59e0b"></i><span>峰段</span><b>${it.peak.toFixed(2)} kWh</b></div>
        <div class="tr"><i style="background:#60a5fa"></i><span>平段</span><b>${it.flat.toFixed(2)} kWh</b></div>
        <div class="tr"><i style="background:#34d399"></i><span>谷段</span><b>${it.valley.toFixed(2)} kWh</b></div>`);
    }
  };
  canvas.onmouseleave = () => {
    const st = canvas._chartState; if (!st) return;
    st.cursor = null; st.hover = null;
    if (kind === 'line') paintChart(canvas); else paintStackedBar(canvas, null);
    hideChartTip();
  };
}

function touRateOf(h) {
  const hit = spans => (spans || []).some(([a, b]) => (a <= b ? h >= a && h < b : h >= a || h < b));
  if (!S.touSpans) return 'flat';
  if (hit(S.touSpans.peak)) return 'peak';
  if (hit(S.touSpans.flat)) return 'flat';
  return 'valley';
}

/* ---------- 恒压供水实验台 ---------- */
function updateWater() {
  const st = S.devices['ws-pump']; if (!st) return;
  const set = (id, v) => { const el = document.getElementById(id); if (el) el.textContent = v; };
  set('wsPressureVal', f2(st.pressure)); set('wsSpVal', 'SP ' + f2(st.sp));
  set('wsFreqVal', f1(st.f) + ' Hz'); set('wsFlowVal', f1(st.flow)); set('wsPowerVal', f1(st.p));
  // PI 闭环：SP 设定 / PV 反馈 / Err 偏差
  set('piSp', f2(st.sp)); set('piPv', f2(st.pressure)); set('piErr', f2((st.sp || 0) - (st.pressure || 0)));
  // 圆形压力表盘指针（0 ~ 0.6 MPa → -120° ~ +120°）
  const needle = document.getElementById('wsNeedle');
  if (needle) {
    const p = Math.max(0, Math.min(0.6, st.pressure || 0));
    needle.setAttribute('transform', `rotate(${(-120 + p / 0.6 * 240).toFixed(1)} 760 92)`);
  }
  const dot = document.getElementById('wsDot'); if (dot) dot.classList.toggle('bad', !comm(st));
  const imp = document.getElementById('wsImpeller');
  if (imp && st.f > 5) imp.setAttribute('transform', `rotate(${(Date.now() / (3000 / st.f * 50)) % 360} 330 205)`);
  const lvl = document.getElementById('wsTankLevel');
  if (lvl) {
    const h = Math.max(20, Math.min(82, 82 - (st.f - 20) * 1.2));
    lvl.setAttribute('y', 222 - h); lvl.setAttribute('height', h);
    const wave = document.getElementById('wsWave');
    if (wave) wave.setAttribute('d', `M48 ${222 - h} q14 -5 28 0 t28 0 t28 0 t28 0`);
  }
}
async function loadWaterTrend() {
  // 图1：压力(左轴 MPa) + 流量(右轴 m³/h)；图2：频率(左轴 Hz) + 泵功率(右轴 kW)
  const q = (metric) => api(`/api/history?devices=ws-pump&metric=${metric}&hours=1&max=360`);
  const [dp, dflow, df, dpow] = await Promise.all([q('pressure'), q('flow'), q('f'), q('p')]);
  const sP = [
    { name: '压力 MPa', color: '#38bdf8', data: dp.series['ws-pump'] || [] },
    { name: '流量 m³/h', color: '#34d399', axis: 'right', data: dflow.series['ws-pump'] || [] },
  ];
  const sF = [
    { name: '频率 Hz', color: '#f59e0b', data: df.series['ws-pump'] || [] },
    { name: '泵功率 kW', color: '#f43f5e', axis: 'right', data: dpow.series['ws-pump'] || [] },
  ];
  drawChart(document.getElementById('wsCanvasP'), sP, { thresholds: THRESHOLDS.pressure });
  drawChart(document.getElementById('wsCanvasF'), sF);
  const legend = (host, arr) => {
    const el = document.getElementById(host);
    if (el) el.innerHTML = arr.map(s => `<span><span class="key" style="background:${s.color}"></span>${s.name}${s.axis === 'right' ? ' (右轴)' : ''}</span>`).join('');
  };
  legend('wsLegendP', sP); legend('wsLegendF', sF);
}

/* ---------- 态势栏 / KPI / SOE ---------- */
function updateStatusbar() {
  const power = ['meter-main', ...FEEDERS];
  const online = power.filter(id => S.devices[id] && comm(S.devices[id])).length;
  const el = document.getElementById('sbOnline');
  if (el) {
    el.innerHTML = `<svg width="10" height="10" viewBox="0 0 10 10" style="vertical-align:middle;margin-inline-end:5px"><circle cx="5" cy="5" r="4" fill="${online === power.length ? '#10B981' : '#F59E0B'}"/></svg><b class="num">${online}/${power.length}</b>`;
    el.title = t('sb.online');
  }
  const f = document.getElementById('sbFreq');
  if (f && S.devices['meter-main']) f.textContent = f1(S.devices['meter-main'].f) + ' Hz';
}
const KPI_ICONS = {
  totalp: '<path d="M13 2L4 14h6l-1 8 9-12h-6l1-8Z"/>',
  today: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 9h18M8 13h3M8 17h6"/>',
  loadrate: '<circle cx="12" cy="12" r="9"/><path d="M12 12l4-4"/><path d="M12 3v2M21 12h-2"/>',
  pf: '<path d="M3 12h4l2-6 4 12 2-6h6"/>',
  imbalance: '<path d="M4 18V9M10 18V5M16 18v-7M22 18v-4"/>',
  thd: '<path d="M2 12h3l2-7 3 14 3-11 2 7 2-4h5"/>',
};
function kpiCard(id, key, unit) {
  return `<div class="kpi">
    <div class="kpi-head"><span class="t"><svg class="kpi-ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">${KPI_ICONS[id]}</svg>${t(key)}</span><span class="kpi-pill" id="kpill-${id}">--</span></div>
    <div class="v num"><span id="kpi-${id}">--</span> <span class="unit">${unit}</span></div>
    <div class="kpi-micro" id="kmicro-${id}"></div>
  </div>`;
}
function buildKpis() {
  const wrap = document.getElementById('kpiRow');
  if (!wrap) return;
  wrap.innerHTML = [
    ['totalp', 'kpi.totalp', 'kW'], ['today', 'kpi.today', 'kWh'],
    ['loadrate', 'kpi.loadrate', '%'], ['pf', 'kpi.pf', ''],
    ['imbalance', 'kpi.imbalance', '%'], ['thd', 'kpi.thd', '%'],
  ].map(([id, key, unit]) => kpiCard(id, key, unit)).join('');
  // 静态微组件骨架
  document.getElementById('kmicro-totalp').innerHTML = '<canvas id="sparkP" height="28"></canvas>';
  document.getElementById('kmicro-today').innerHTML = '<div class="tou-mini" id="touMini"><i style="background:#f59e0b;width:33%"></i><i style="background:#60a5fa;width:33%"></i><i style="background:#34d399;width:34%"></i></div><div class="micro-row"><span>峰平谷占比</span><span id="todayCost" class="num">--</span></div>';
  document.getElementById('kmicro-loadrate').innerHTML = '<div class="micro-track"><i id="lrBar" style="width:0;background:linear-gradient(90deg,#10B981,#F59E0B,#EF4444)"></i></div><div class="micro-row"><span>70% 预警 / 85% 告警</span><span class="muted">250kVA·400A</span></div>';
  document.getElementById('kmicro-pf').innerHTML = '<div class="micro-scale"><span class="mark" id="pfMark" style="left:80%"></span></div><div class="micro-row"><span>0.50</span><span style="color:#6EE7B7">0.90 考核线</span><span>1.00</span></div>';
  document.getElementById('kmicro-imbalance').innerHTML = '<div class="micro-mini" id="phaseMini"><i></i><i></i><i></i></div><div class="micro-row"><span>Ua/Ub/Uc</span><span id="imbCaps" class="kpi-pill ok">GB/T &lt;2%</span></div>';
  document.getElementById('kmicro-thd').innerHTML = '<div class="micro-track"><i id="thdBar" style="width:0;background:linear-gradient(90deg,#10B981,#F59E0B,#EF4444)"></i></div><div class="micro-row"><span>0%</span><span id="thdCaps" class="kpi-pill ok">IEEE 519 &lt;5%</span><span>8%</span></div>';
}
const setPill = (id, text, cls) => { const el = document.getElementById('kpill-' + id); if (el) { el.textContent = text; el.className = 'kpi-pill ' + cls; } };
function updateKpis() {
  const m = S.devices['meter-main']; if (!m) return;
  const set = (id, v) => { const el = document.getElementById('kpi-' + id); if (el) el.textContent = v; };
  set('totalp', f1(m.p)); set('loadrate', f1(m.load_rate)); set('pf', f2(m.pf));
  set('imbalance', f1(m.imbalance)); set('thd', f1(m.thd));
  if (S.todayKwh != null) set('today', f1(S.todayKwh));
  // 状态胶囊
  const lr = m.load_rate || 0, pf = m.pf || 0, imb = m.imbalance || 0, thd = m.thd || 0;
  setPill('totalp', lr > 85 ? '过载' : '正常', lr > 85 ? 'bad' : 'ok');
  setPill('loadrate', lr > 85 ? '过载' : lr > 70 ? '偏高' : '正常', lr > 85 ? 'bad' : lr > 70 ? 'warn' : 'ok');
  setPill('pf', pf < 0.9 ? '偏低' : '达标', pf < 0.9 ? 'warn' : 'ok');
  setPill('imbalance', imb < 2 ? '合格' : '超标', imb < 2 ? 'ok' : 'warn');
  setPill('thd', thd < 5 ? '合格' : '超标', thd < 5 ? 'ok' : 'warn');
  setPill('today', '累计', 'ok');
  // 微可视化
  const lrBar = document.getElementById('lrBar'); if (lrBar) lrBar.style.width = Math.min(100, lr) + '%';
  const thdBar = document.getElementById('thdBar'); if (thdBar) thdBar.style.width = Math.min(100, thd / 8 * 100) + '%';
  const pfMark = document.getElementById('pfMark'); if (pfMark) pfMark.style.left = Math.max(0, Math.min(100, (pf - 0.5) / 0.5 * 100)) + '%';
  const mini = document.querySelectorAll('#phaseMini i');
  const ph = [m.ua || 0, m.ub || 0, m.uc || 0], mx = Math.max(...ph, 1);
  mini.forEach((el, i) => { el.style.height = (ph[i] / mx * 24) + 'px'; });
  const ic = document.getElementById('imbCaps'); if (ic) { ic.textContent = imb < 2 ? 'GB/T <2% 合格' : '超标'; ic.className = 'kpi-pill ' + (imb < 2 ? 'ok' : 'warn'); }
  const tc = document.getElementById('thdCaps'); if (tc) { tc.textContent = thd < 5 ? 'IEEE 519 <5%' : '超标'; tc.className = 'kpi-pill ' + (thd < 5 ? 'ok' : 'warn'); }
  // 今日峰平谷占比 + 预估电费
  const td = S.todayBuckets;
  if (td) {
    const tot = (td.peak_kwh || 0) + (td.flat_kwh || 0) + (td.valley_kwh || 0) || 1;
    const bars = document.querySelectorAll('#touMini i');
    if (bars[0]) bars[0].style.width = (td.peak_kwh / tot * 100) + '%';
    if (bars[1]) bars[1].style.width = (td.flat_kwh / tot * 100) + '%';
    if (bars[2]) bars[2].style.width = (td.valley_kwh / tot * 100) + '%';
    const c = document.getElementById('todayCost');
    if (c) c.textContent = '¥' + (td.cost || 0).toFixed(2);
  }
}
function renderSpark() {
  const cv = document.getElementById('sparkP'); if (!cv || S.hist.length < 2) return;
  const dpr = window.devicePixelRatio || 1;
  const W = cv.clientWidth || 160, H = 28;
  cv.width = W * dpr; cv.height = H * dpr;
  const ctx = cv.getContext('2d'); ctx.scale(dpr, dpr); ctx.clearRect(0, 0, W, H);
  const mn = Math.min(...S.hist), mx = Math.max(...S.hist), rg = (mx - mn) || 1;
  const pts = S.hist.map((v, i) => [i / (S.hist.length - 1) * (W - 2) + 1, H - 3 - (v - mn) / rg * (H - 6)]);
  const g = ctx.createLinearGradient(0, 0, 0, H);
  g.addColorStop(0, 'rgba(6,182,212,.35)'); g.addColorStop(1, 'rgba(6,182,212,0)');
  ctx.beginPath(); pts.forEach((p, i) => i ? ctx.lineTo(p[0], p[1]) : ctx.moveTo(p[0], p[1]));
  ctx.lineTo(pts[pts.length - 1][0], H); ctx.lineTo(pts[0][0], H); ctx.closePath();
  ctx.fillStyle = g; ctx.fill();
  ctx.beginPath(); pts.forEach((p, i) => i ? ctx.lineTo(p[0], p[1]) : ctx.moveTo(p[0], p[1]));
  ctx.strokeStyle = '#22D3EE'; ctx.lineWidth = 1.6; ctx.stroke();
}
async function fetchToday() {
  try {
    const d = await api('/api/report/day');
    S.todayKwh = d.totals.total_kwh;
    S.todayBuckets = d.totals;
    updateKpis();
  } catch (e) { /* 忽略 */ }
}
function soePush(a) {
  S.soe.unshift(Object.assign({}, a));
  if (S.soe.length > 40) S.soe.pop();
  renderSoe(); updateStatusbarSev();
}
function updateStatusbarSev() {
  const el = document.getElementById('sbSev');
  if (!el) return;
  const rank = { critical: 3, warning: 2, info: 1 };
  const open = S.soe.filter(x => x.status === 'active' || x.status === 'acked');
  const top = open.sort((x, y) => (rank[y.severity] || 0) - (rank[x.severity] || 0))[0];
  if (!top) { el.textContent = 'NORMAL'; el.className = 'badge sb'; return; }
  el.innerHTML = `<span class="pulse">●</span> ${t('alarm.s' + top.severity)} ×${open.length}`;
  el.className = 'badge sb sev ' + top.severity;
}
function renderSoe() {
  const host = document.getElementById('soeList'); if (!host) return;
  if (!S.soe.length) { host.innerHTML = `<p class="muted">${t('soe.empty')}</p>`; return; }
  const dotc = { critical: '#EF4444', warning: '#F59E0B', info: '#60A5FA' };
  host.innerHTML = S.soe.map(a => `
    <div class="soe-row">
      <span class="soe-dot ${a.status === 'active' ? 'pulse' : ''}" style="background:${dotc[a.severity] || '#94A3B8'}"></span>
      <span class="when num">${fdate(new Date(a.triggered_at).getTime())}</span>
      <span class="sev ${a.severity}">${t('alarm.s' + a.severity)}</span>
      <span class="grow">${esc(devName(a.device_id))} · ${esc(locMsg(a))}</span>
      <span class="st ${a.status}">${t('alarm.' + a.status)}</span>
      ${a.status === 'active' ? `<button class="btn sm" onclick="ackSoe(${a.id})">${t('alarm.ack')}</button>` : ''}
    </div>`).join('');
  updateStatusbarSev();
}
async function ackSoe(id) { await api(`/api/alarms/${id}/ack`, { method: 'POST' }); const it = S.soe.find(x => x.id === id); if (it) it.status = 'acked'; renderSoe(); }
/* ---------- 遥控二次确认（选择—返校—执行） ---------- */
let MODAL = null;
function openBreakerModal(id, on) {
  MODAL = { device: id, on };
  const act = on ? 'breaker_close' : 'breaker_open';
  document.getElementById('modalInfo').textContent = `${act} → ${devName(id)}`;
  document.getElementById('modalCheck').textContent = t('modal.returncheck').replaceAll('{action}', act).replaceAll('{device}', devName(id));
  document.getElementById('modalReason').value = '';
  document.getElementById('modal').hidden = false;
}
function closeBreakerModal() { document.getElementById('modal').hidden = true; MODAL = null; }

/* ---------- 能耗报表 ---------- */
function renderEnergySummary(d) {
  const host = document.getElementById('energySummary'); if (!host) return;
  if (S.energyView === 'day') {
    const tt = d.totals || {};
    const peak = (tt.peak_kwh || 0) * (d.tariff?.peak || 0);
    const flat = (tt.flat_kwh || 0) * (d.tariff?.flat || 0);
    const valley = (tt.valley_kwh || 0) * (d.tariff?.valley || 0);
    const cards = [
      ['总电量', `${(tt.total_kwh || 0).toFixed(1)}<small class="unit"> kWh</small>`, '当日累计用电', ''],
      ['峰段电费', `¥${peak.toFixed(2)}`, `${(tt.peak_kwh || 0).toFixed(1)} kWh · ¥${d.tariff?.peak || 0}/kWh`, 'peak'],
      ['平段电费', `¥${flat.toFixed(2)}`, `${(tt.flat_kwh || 0).toFixed(1)} kWh · ¥${d.tariff?.flat || 0}/kWh`, 'flat'],
      ['谷段电费', `¥${valley.toFixed(2)}`, `${(tt.valley_kwh || 0).toFixed(1)} kWh · ¥${d.tariff?.valley || 0}/kWh`, 'valley'],
    ];
    host.innerHTML = cards.map(([k, v, sub, cls]) => `<div class="sumcard ${cls}"><div class="t">${k}</div><div class="v num">${v}</div><div class="sub">${sub}</div></div>`).join('');
  } else {
    const days = d.days || [];
    const tot = days.reduce((a, b) => a + b.total_kwh, 0);
    const cost = days.reduce((a, b) => a + b.cost, 0);
    const avg = days.length ? tot / days.length : 0;
    const maxDay = days.reduce((a, b) => (b.total_kwh > (a.total_kwh || 0) ? b : a), {});
    host.innerHTML = [
      ['月累计电量', `${tot.toFixed(1)}<small class="unit"> kWh</small>`, `${days.length} 天数据`, ''],
      ['月累计电费', `¥${cost.toFixed(2)}`, '按分时电价核算', 'peak'],
      ['日均用电', `${avg.toFixed(1)}<small class="unit"> kWh</small>`, '当月平均', 'flat'],
      ['峰值日', `${(maxDay.total_kwh || 0).toFixed(1)}<small class="unit"> kWh</small>`, maxDay.date || '--', 'valley'],
    ].map(([k, v, sub, cls]) => `<div class="sumcard ${cls}"><div class="t">${k}</div><div class="v num">${v}</div><div class="sub">${sub}</div></div>`).join('');
  }
}
function exportCsv() {
  const d = S.energyData; if (!d) return toast('暂无可导出数据', 'warn');
  let rows;
  if (S.energyView === 'day') {
    rows = [['设备', '总电量(kWh)', '峰段(kWh)', '平段(kWh)', '谷段(kWh)', '电费']];
    (d.devices || []).forEach(v => rows.push([v.name, v.total_kwh, v.peak_kwh, v.flat_kwh, v.valley_kwh, v.cost]));
    const tt = d.totals || {};
    rows.push(['合计', tt.total_kwh, tt.peak_kwh, tt.flat_kwh, tt.valley_kwh, tt.cost]);
  } else {
    rows = [['日期', '电量(kWh)', '电费']];
    (d.days || []).forEach(v => rows.push([v.date, v.total_kwh, v.cost]));
  }
  const csv = '\uFEFF' + rows.map(r => r.map(x => `"${String(x ?? '').replaceAll('"', '""')}"`).join(',')).join('\r\n');
  const blob = new Blob([csv], { type: 'text/csv;charset=utf-8' });
  const a = document.createElement('a');
  a.href = URL.createObjectURL(blob);
  a.download = `energy-${S.energyView}-${document.getElementById('energyDate').value || 'report'}.csv`;
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 1000);
  toast('CSV 已导出');
}
async function loadEnergy() {
  const dateEl = document.getElementById('energyDate');
  if (!dateEl.value) dateEl.valueAsDate = new Date();
  const date = dateEl.value;
  const host = document.getElementById('energyTable');
  const canvas = document.getElementById('energyCanvas');
  if (S.energyView === 'day') {
    const d = await api(`/api/report/day?date=${date}`);
    S.energyData = d;
    renderEnergySummary(d);
    document.getElementById('energyTariff').textContent =
      `${t('energy.peak')}${d.tariff.peak} / ${t('energy.flat')}${d.tariff.flat} / ${t('energy.valley')}${d.tariff.valley} ${d.currency === 'CNY' ? '¥/kWh' : '$/kWh'}`;
    const rows = d.devices.map(v => `<tr>
      <td>${t('dev.' + v.id, null) || v.name}</td><td>${v.total_kwh}</td><td>${v.peak_kwh}</td>
      <td>${v.flat_kwh}</td><td>${v.valley_kwh}</td><td>${v.cost}</td></tr>`);
    host.innerHTML = `<table><thead><tr><th>${t('energy.device')}</th><th>kWh ${t('energy.total')}</th>
      <th>${t('energy.peak')}</th><th>${t('energy.flat')}</th><th>${t('energy.valley')}</th><th>${t('energy.cost')}</th></tr></thead><tbody>
      ${rows.join('')}<tr style="font-weight:700"><td>${t('energy.totals')}</td><td>${d.totals.total_kwh}</td>
      <td>${d.totals.peak_kwh}</td><td>${d.totals.flat_kwh}</td><td>${d.totals.valley_kwh}</td><td>${d.totals.cost}</td></tr></tbody></table>`;
    drawStackedBar(canvas, d.devices.map(v => ({ name: t('dev.' + v.id) || v.name, peak: v.peak_kwh, flat: v.flat_kwh, valley: v.valley_kwh })));
  } else {
    const month = date.slice(0, 7);
    const d = await api(`/api/report/month?month=${month}`);
    S.energyData = d;
    renderEnergySummary(d);
    const rows = d.days.map(v => `<tr><td>${v.date}</td><td>${v.total_kwh}</td><td>${v.cost}</td></tr>`).join('');
    host.innerHTML = `<table><thead><tr><th>${t('energy.date')}</th><th>kWh</th><th>${t('energy.cost')}</th></tr></thead><tbody>${rows}</tbody></table>`;
    drawChart(canvas, [{ name: 'kWh', color: '#22d3ee', data: d.days.map(v => [new Date(v.date).getTime(), v.total_kwh]) }]);
  }
}
function drawStackedBar(canvas, items) {
  const dpr = window.devicePixelRatio || 1;
  if (!canvas.dataset.h) canvas.dataset.h = canvas.getAttribute('height');
  const W = canvas.clientWidth || 900, H = +canvas.dataset.h;
  canvas._chartState = { W, H, dpr, items, L: 40, R: 10, T: 10, B: 26, hover: null };
  paintStackedBar(canvas, null);
  bindChartInteraction(canvas, 'bar');
}
function paintStackedBar(canvas, hover) {
  const st = canvas._chartState; if (!st) return;
  const { W, H, dpr, items, L, R, T, B } = st;
  canvas.width = W * dpr; canvas.height = H * dpr;
  const ctx = canvas.getContext('2d'); ctx.scale(dpr, dpr); ctx.clearRect(0, 0, W, H);
  if (!items.length) return;
  const max = Math.max(...items.map(i => i.peak + i.flat + i.valley)) || 1;
  const bw = Math.min(90, (W - L - R) / items.length * 0.55);
  const colors = { peak: '#f59e0b', flat: '#60a5fa', valley: '#34d399' };
  ctx.font = '11px ui-monospace, monospace';
  items.forEach((it, i) => {
    const cx = L + (i + 0.5) * (W - L - R) / items.length;
    if (hover === i) {
      ctx.fillStyle = 'rgba(34,211,238,.08)';
      ctx.fillRect(cx - bw / 2 - 8, T, bw + 16, H - T - B);
    }
    let y = H - B;
    for (const k of ['peak', 'flat', 'valley']) {
      const h = it[k] / max * (H - T - B);
      ctx.fillStyle = colors[k];
      if (hover === i) { ctx.shadowColor = colors[k]; ctx.shadowBlur = 10; }
      ctx.fillRect(cx - bw / 2, y - h, bw, h); y -= h;
      ctx.shadowBlur = 0;
    }
    ctx.fillStyle = hover === i ? '#e2e8f0' : '#94a3b8';
    ctx.fillText(it.name.slice(0, 8), cx - 24, H - 8);
    ctx.fillText((it.peak + it.flat + it.valley).toFixed(1), cx - 14, y - 4);
  });
}

/* ---------- 告警 ---------- */
async function loadAlarmSummary() {
  try {
    const d = await api('/api/alarms?status=all&limit=500');
    const a = d.alarms || [];
    const act = a.filter(x => x.status === 'active').length;
    const ack = a.filter(x => x.status === 'acked').length;
    const closed = a.filter(x => x.status === 'closed').length;
    const today = a.filter(x => new Date(x.triggered_at).toDateString() === new Date().toDateString()).length;
    const host = document.getElementById('alarmSummary');
    if (host) host.innerHTML = [
      ['未确认告警', act, '需立即处理', 'bad'],
      ['已确认', ack, '等待关闭', 'warn'],
      ['今日新增', today, '24h 内触发', ''],
      ['已关闭', closed, '历史归档', 'ok'],
    ].map(([k, v, sub, cls]) => `<div class="sumcard"><div class="t">${k}</div><div class="v num ${cls}" style="color:${cls === 'bad' ? '#FCA5A5' : cls === 'warn' ? '#FCD34D' : cls === 'ok' ? '#6EE7B7' : 'var(--tx)'}">${v}</div><div class="sub">${sub}</div></div>`).join('');
  } catch (e) { /* 忽略 */ }
}
async function loadAlarms() {
  loadAlarmSummary();
  const d = await api(`/api/alarms?status=${S.alarmFilter}&limit=100`);
  const host = document.getElementById('alarmTable');
  if (!d.alarms.length) { host.innerHTML = `<p class="muted">${t('alarm.empty')}</p>`; return; }
  host.innerHTML = `<table><thead><tr><th>${t('alarm.triggered_at')}</th><th>${t('alarm.sev')}</th><th>${t('dev.title')}</th>
    <th>${t('alarm.value')}</th><th>${t('common.status')}</th><th></th></tr></thead><tbody>` +
    d.alarms.map(a => `<tr>
      <td>${fdate(new Date(a.triggered_at).getTime())}</td>
      <td><span class="sev ${a.severity}">${t('alarm.s' + a.severity)}</span></td>
      <td>${esc(devName(a.device_id))}</td>
      <td>${esc(locMsg(a))}${a.metric !== 'comm' ? ` <b>(${f1(a.value)})</b>` : ''}</td>
      <td><span class="st ${a.status}">${t('alarm.' + a.status)}</span></td>
      <td>${a.status === 'active' ? `<button class="btn sm" onclick="ackAlarm(${a.id})">${t('alarm.ack')}</button> ` : ''}
          ${a.status !== 'closed' ? `<button class="btn sm" onclick="closeAlarm(${a.id})">${t('alarm.close')}</button>
          <button class="btn sm" onclick="toWo(${a.id}, ${JSON.stringify(locMsg(a)).replaceAll('"', '&quot;')})">${t('alarm.towo')}</button>` : ''}</td>
    </tr>`).join('') + '</tbody></table>';
}
async function ackAlarm(id) { await api(`/api/alarms/${id}/ack`, { method: 'POST' }); loadAlarms(); }
async function closeAlarm(id) { await api(`/api/alarms/${id}/close`, { method: 'POST' }); loadAlarms(); }
async function toWo(alarmId, title) {
  document.getElementById('woTitle').value = title;
  await api('/api/workorders', { method: 'POST', body: JSON.stringify({ title, alarm_id: alarmId, detail: 'from alarm #' + alarmId }) });
  toast(t('common.saved')); switchTab('workorders');
}

/* ---------- 工单 ---------- */
async function loadWos() {
  const d = await api('/api/workorders');
  const host = document.getElementById('woTable');
  const wos = d.workorders || [];
  const sum = document.getElementById('woSummary');
  if (sum) sum.innerHTML = [
    ['工单总数', wos.length, '全部工单', ''],
    ['待处理', wos.filter(w => w.status === 'open').length, 'open', 'warn'],
    ['处理中', wos.filter(w => w.status === 'processing').length, 'processing', 'bad'],
    ['已闭环', wos.filter(w => w.status === 'closed').length, 'closed', 'ok'],
  ].map(([k, v, sub, cls]) => `<div class="sumcard"><div class="t">${k}</div><div class="v num" style="color:${cls === 'warn' ? '#FCD34D' : cls === 'bad' ? '#FCA5A5' : cls === 'ok' ? '#6EE7B7' : 'var(--tx)'}">${v}</div><div class="sub">${sub}</div></div>`).join('');
  if (!wos.length) { host.innerHTML = `<p class="muted">${t('wo.empty')}</p>`; return; }
  host.innerHTML = `<table><thead><tr><th>#</th><th>${t('wo.title')}</th><th>${t('common.status')}</th>
    <th>${t('wo.assignee')}</th><th>${t('wo.created')}</th><th>${t('wo.note')}</th><th></th></tr></thead><tbody>` +
    wos.map(w => {
      const notes = w.notes || [];
      const timeline = `<div class="wt-timeline">
        <div class="wt-node"><b>${fdate(new Date(w.created_at).getTime())}</b> 工单创建</div>
        ${notes.map(n => `<div class="wt-node"><span class="who">${esc(n.author || 'ops')}</span><b>${esc(n.at || '')}</b> ${esc(n.note)}</div>`).join('') || '<div class="wt-node muted">暂无处理记录</div>'}
      </div>`;
      return `<tr>
      <td>${w.id}</td><td>${esc(w.title)}${w.alarm_id ? ` <span class="sev ${w.severity || 'info'}">A#${w.alarm_id}</span>` : ''}</td>
      <td><span class="st ${w.status}">${t('wo.st' + w.status)}</span></td>
      <td>${esc(w.assignee) || '--'}</td>
      <td>${fdate(new Date(w.created_at).getTime())}</td>
      <td>${timeline}<div style="margin-top:6px;display:flex;gap:5px"><input id="wn-${w.id}" size="12" placeholder="添加处理记录"> <button class="btn sm" onclick="addNote(${w.id})">${t('wo.addnote')}</button></div></td>
      <td>${w.status === 'open' ? `<button class="btn sm" onclick="woSt(${w.id},'processing')">${t('wo.set_processing')}</button> ` : ''}
          ${w.status !== 'closed' ? `<button class="btn sm danger" onclick="woSt(${w.id},'closed')">${t('wo.close_wo')}</button>` : ''}</td>
    </tr>`;
    }).join('') + '</tbody></table>';
}
async function addNote(id) {
  const el = document.getElementById('wn-' + id);
  if (!el.value) return;
  await api(`/api/workorders/${id}/note`, { method: 'POST', body: JSON.stringify({ note: el.value }) });
  loadWos();
}
async function woSt(id, st) { await api(`/api/workorders/${id}/status`, { method: 'POST', body: JSON.stringify({ status: st }) }); loadWos(); }

/* ---------- 设备与规则 ---------- */
async function loadDevices() {
  const d = await api('/api/devices');
  document.getElementById('devTable').innerHTML = `<table><thead>
    <tr><th>${t('dev.name')}</th><th>Kind</th><th>Driver</th><th>${t('dev.comm')}</th>
    <th>${t('dev.last_ts')}</th><th>P kW</th><th>EPi kWh</th><th>Modbus</th></tr></thead><tbody>` +
    d.devices.map(v => `<tr>
      <td>${devName(v.id, v.name)} <span class="muted">${esc(v.id)}</span></td>
      <td>${v.kind}</td><td><code>${v.driver}</code></td>
      <td><span class="dot ${v.comm ? '' : 'bad'}"></span></td>
      <td>${v.last_ts ? ftime(v.last_ts) : '--'}</td>
      <td>${f1(v.p)}</td><td>${f1(v.epi)}</td>
      <td class="muted">${(v.metrics || []).map(m => `${m.metric}:0x${m.reg ?? '--'}`).join(' ')}</td>
    </tr>`).join('') + '</tbody></table>';
  const sel = document.getElementById('ruleDev');
  sel.innerHTML = d.devices.map(v => `<option value="${esc(v.id)}">${devName(v.id, v.name)}</option>`).join('');
}
async function loadRules() {
  const d = await api('/api/rules');
  document.getElementById('ruleTable').innerHTML = `<table><thead>
    <tr><th>#</th><th>${t('dev.name')}</th><th>${t('trend.metric')}</th><th>${t('dev.cond')}</th>
    <th>${t('dev.duration')}s</th><th>${t('dev.hysteresis')}</th><th>${t('dev.severity')}</th><th></th></tr></thead><tbody>` +
    d.rules.map(r => `<tr>
      <td>${r.id}</td><td>${t('dev.' + r.device_id)}</td><td>${r.metric}</td>
      <td>${r.op === 'gt' ? '&gt;' : '&lt;'} ${r.threshold}</td>
      <td>${r.duration_s}</td><td>${r.hysteresis}</td>
      <td><span class="sev ${r.severity}">${r.severity}</span></td>
      <td><label class="switch" title="${r.enabled ? '已启用' : '已停用'}"><input type="checkbox" ${r.enabled ? 'checked' : ''} onchange="toggleRule(${r.id})"><span class="slider"></span></label>
      <button class="btn sm danger" onclick="delRule(${r.id})" title="删除规则">✕</button></td>
    </tr>`).join('') + '</tbody></table>';
}
async function toggleRule(id) { await api(`/api/rules/${id}/toggle`, { method: 'POST' }); loadRules(); }
async function delRule(id) { await api(`/api/rules/${id}`, { method: 'DELETE' }); loadRules(); }

/* ---------- 对接 ---------- */
async function loadSettings() {
  const s = await api('/api/settings');
  document.getElementById('whUrl').value = s.webhook_url || '';
  document.getElementById('apiTokenInput').value = localStorage.getItem('es_token') || '';
  document.getElementById('tarPeak').value = s.tou.tariff.peak;
  document.getElementById('tarFlat').value = s.tou.tariff.flat;
  document.getElementById('tarValley').value = s.tou.tariff.valley;
  loadApiDoc();
}
function loadApiDoc() {
  const base = location.origin;
  const text = `# ${t('api.doc.latest')}
curl -H "X-API-Token: $TOKEN" ${base}/api/v1/open/latest

# ${t('api.doc.history')}
curl -H "X-API-Token: $TOKEN" "${base}/api/v1/open/history?devices=feeder-ac&metric=p&hours=8"

# ${t('api.doc.alarms')}
curl -H "X-API-Token: $TOKEN" ${base}/api/v1/open/alarms?status=active

# ${t('api.doc.ingest')}
curl -X POST -H "X-API-Token: $TOKEN" -H "Content-Type: application/json" \\
  -d '{"device":"ext-temp-01","metric":"temp","value":26.5}' ${base}/api/v1/open/ingest

# ${t('api.doc.export')}
curl -H "X-API-Token: $TOKEN" "${base}/api/v1/open/export/day?date=$(date +%F)" -o energy.csv

# ${t('api.doc.webhook')}
POST {webhook_url}  {"event":"alarm.triggered","alarm":{...}}`;
  const el = document.getElementById('apiDoc');
  if (!el) return;
  el._raw = text;
  const H = s => esc(s);
  el.innerHTML = text.split('\n').map(line => {
    if (line.startsWith('# ')) return `<span class="cm">${H(line)}</span>`;
    if (line.startsWith('curl')) {
      return line.replace(/^(curl)(.*)$/, (_, c, rest) =>
        `<span class="kw">${c}</span>` + H(rest).replace(/(https?:\/\/[^" ]+)/g, '<span class="u">$1</span>').replace(/(\$TOKEN)/g, '<span class="v">$1</span>'));
    }
    return H(line);
  }).join('\n');
}
function copyApiDoc() {
  const el = document.getElementById('apiDoc');
  const text = el?._raw || el?.textContent || '';
  navigator.clipboard.writeText(text).then(() => toast('API 命令已复制')).catch(() => toast('复制失败，请手动选择', 'warn'));
}

/* ---------- 绑定 ---------- */
function bind() {
  bindPanelTabs();
  const csvBtn = document.getElementById('btnCsv'); if (csvBtn) csvBtn.onclick = exportCsv;
  const copyBtn = document.getElementById('btnCopyApi'); if (copyBtn) copyBtn.onclick = copyApiDoc;
  document.querySelectorAll('nav#tabs button').forEach(b => b.onclick = () => switchTab(b.dataset.tab));
  document.getElementById('bellBtn').onclick = () => switchTab('alarms');
  document.getElementById('langSel').onchange = e => loadLang(e.target.value);
  document.getElementById('trendMetric').onchange = e => { S.metric = e.target.value; loadTrend(); };
  document.querySelectorAll('.range').forEach(b => b.onclick = () => {
    document.querySelectorAll('.range').forEach(x => x.classList.remove('active'));
    b.classList.add('active'); S.hours = +b.dataset.h; loadTrend();
  });
  document.querySelectorAll('.fstatus').forEach(b => b.onclick = () => {
    document.querySelectorAll('.fstatus').forEach(x => x.classList.remove('active'));
    b.classList.add('active'); S.alarmFilter = b.dataset.s; loadAlarms();
  });
  document.querySelectorAll('.view').forEach(b => b.onclick = () => {
    document.querySelectorAll('.view').forEach(x => x.classList.remove('active'));
    b.classList.add('active'); S.energyView = b.dataset.v; loadEnergy();
  });
  document.getElementById('energyDate').onchange = loadEnergy;
  document.getElementById('woCreate').onclick = async () => {
    const title = document.getElementById('woTitle').value.trim();
    if (!title) return toast(t('wo.title'), 'warn');
    await api('/api/workorders', {
      method: 'POST',
      body: JSON.stringify({ title, assignee: document.getElementById('woAssignee').value.trim() || null }),
    });
    document.getElementById('woTitle').value = '';
    loadWos(); toast(t('common.saved'));
  };
  document.getElementById('ruleAdd').onclick = async () => {
    await api('/api/rules', {
      method: 'POST',
      body: JSON.stringify({
        device_id: document.getElementById('ruleDev').value,
        metric: document.getElementById('ruleMetric').value,
        op: document.getElementById('ruleOp').value,
        threshold: +document.getElementById('ruleThreshold').value,
        duration_s: +document.getElementById('ruleDur').value,
        hysteresis: +document.getElementById('ruleHyst').value,
        severity: document.getElementById('ruleSev').value,
      }),
    });
    loadRules(); toast(t('common.saved'));
  };
  document.getElementById('btnOverIm').onclick = () => api('/api/sim/cmd', { method: 'POST', body: JSON.stringify({ action: 'overload', device: 'feeder-im', kw: 40, secs: 120 }) }).then(() => toast('+40kW → feeder-im', 'warn'));
  document.getElementById('btnOverAc').onclick = () => api('/api/sim/cmd', { method: 'POST', body: JSON.stringify({ action: 'overload', device: 'feeder-ac', kw: 20, secs: 120 }) }).then(() => toast('+20kW → feeder-ac', 'warn'));
  document.getElementById('btnComm').onclick = () => api('/api/sim/cmd', { method: 'POST', body: JSON.stringify({ action: 'comm_fault', device: 'meter-main', secs: 45 }) }).then(() => toast('comm fault → meter-main', 'warn'));
  document.getElementById('btnClear').onclick = () => api('/api/sim/cmd', { method: 'POST', body: JSON.stringify({ action: 'clear' }) }).then(() => toast('cleared'));
  document.getElementById('wsApply').onclick = () => api('/api/sim/cmd', { method: 'POST', body: JSON.stringify({ action: 'water_sp', sp: +document.getElementById('wsSpInput').value }) }).then(() => toast('SP → ' + document.getElementById('wsSpInput').value + ' MPa'));
  document.getElementById('wsDemand').onclick = () => api('/api/sim/cmd', { method: 'POST', body: JSON.stringify({ action: 'water_demand', pct: 1.0, secs: 90 }) }).then(() => toast('+100% demand 90s', 'warn'));
  document.getElementById('wsNormal').onclick = () => api('/api/sim/cmd', { method: 'POST', body: JSON.stringify({ action: 'water_demand', pct: 0, secs: 0.1 }) }).then(() => toast('restored'));
  document.getElementById('whSave').onclick = async () => {
    await api('/api/settings', { method: 'POST', body: JSON.stringify({ webhook_url: document.getElementById('whUrl').value.trim() }) });
    toast(t('common.saved'));
  };
  document.getElementById('whTest').onclick = async () => {
    try { await api('/api/webhook/test', { method: 'POST' }); toast('→ ' + t('api.webhook_test')); }
    catch (e) { toast(e.message, 'warn'); }
  };
  const tokenHeader = () => ({ 'X-API-Token': document.getElementById('apiTokenInput').value.trim() });
  document.getElementById('tokenShow').onclick = async () => {
    try {
      const d = await fetch('/api/settings/token', { headers: tokenHeader() });
      if (!d.ok) return toast(t('api.token_bad') || 'invalid token', 'warn');
      const j = await d.json();
      document.getElementById('apiTokenInput').type = 'text';
      document.getElementById('apiTokenInput').value = j.api_token;
      localStorage.setItem('es_token', j.api_token); loadApiDoc();
    } catch (e) { toast(String(e), 'warn'); }
  };
  document.getElementById('tokenRegen').onclick = async () => {
    try {
      const d = await fetch('/api/settings/token', { method: 'POST', headers: tokenHeader() });
      if (!d.ok) return toast('invalid token', 'warn');
      const j = await d.json();
      document.getElementById('apiTokenInput').value = j.api_token;
      localStorage.setItem('es_token', j.api_token); loadApiDoc(); toast(t('common.saved'));
    } catch (e) { toast(String(e), 'warn'); }
  };
  document.getElementById('tarSave').onclick = async () => {
    await api('/api/settings', {
      method: 'POST',
      body: JSON.stringify({ tariff: { peak: +document.getElementById('tarPeak').value, flat: +document.getElementById('tarFlat').value, valley: +document.getElementById('tarValley').value } }),
    });
    toast(t('common.saved'));
  };
}

/* ---------- 启动 ---------- */
(async function init() {
  bind();
  setSse(false);
  await loadLang(S.lang);
  openSSE();
  api('/api/state').then(s => {
    S.tariff = s.tariff; S.currency = s.currency;
    S.touSpans = s.tou_spans || null;
    if (s.injections) S.injections = s.injections;
    if (s.uptime_s != null) S.uptime_s = s.uptime_s;
    if (s.db_rows != null) S.db_rows = s.db_rows;
    renderInjections(); updateStatusBar();
  }).catch(() => {});
  fetchToday(); setInterval(fetchToday, 60_000);
  loadAudit();
  api('/api/alarms?status=all&limit=40').then(d => { S.soe = d.alarms.reverse(); renderSoe(); }).catch(() => {});
  setInterval(() => {
    const el = document.getElementById('sbClock');
    if (el) el.textContent = new Date().toLocaleTimeString([], { hour12: false });
  }, 1000);
  document.getElementById('modalCancel').onclick = closeBreakerModal;
  document.getElementById('modalExec').onclick = () => {
    if (!MODAL) return closeBreakerModal();
    api('/api/sim/cmd', {
      method: 'POST',
      body: JSON.stringify({ action: 'breaker', device: MODAL.device, on: MODAL.on, reason: document.getElementById('modalReason').value.trim(), user: 'ops' }),
    }).then(() => { closeBreakerModal(); toast(t('common.saved')); }).catch(closeBreakerModal);
  };
  setInterval(() => { if (curTab === 'trend') loadTrend(); }, 60_000);
  setInterval(() => { if (curTab === 'water') loadWaterTrend(); }, 30_000);
})();
