/* EnergySentinel 前端 —— 单文件，无构建链 */
'use strict';

const CFG = {
  'meter-main': { color: '#f43f5e', order: 0 },
  'feeder-ac':  { x: 160, color: '#f59e0b', icon: 'ac', order: 1 },
  'feeder-im':  { x: 360, color: '#06b6d4', icon: 'im', order: 2 },
  'feeder-lt':  { x: 560, color: '#84cc16', icon: 'lt', order: 3 },
  'feeder-it':  { x: 760, color: '#a78bfa', icon: 'it', order: 4 },
  'ws-pump':    { color: '#38bdf8', order: 5, water: true },
};
const FEEDERS = Object.keys(CFG).filter(k => k !== 'meter-main');
const ICONS = {
  ac: c => `<circle cx="${c}" cy="300" r="17" class="svg-load"/><path d="M${c - 8} 309 L${c + 9} 300 L${c - 8} 291 Z" class="svg-load"/>`,
  im: c => `<rect x="${c - 18}" y="284" width="36" height="34" rx="3" class="svg-load"/><line x1="${c - 8}" y1="292" x2="${c - 8}" y2="310" class="svg-load"/><line x1="${c + 8}" y1="292" x2="${c + 8}" y2="310" class="svg-load"/>`,
  lt: c => `<circle cx="${c}" cy="298" r="13" class="svg-load"/><line x1="${c}" y1="285" x2="${c}" y2="279" class="svg-load"/><line x1="${c - 13}" y1="298" x2="${c - 19}" y2="298" class="svg-load"/><line x1="${c + 13}" y1="298" x2="${c + 19}" y2="298" class="svg-load"/>`,
  it: c => `<rect x="${c - 16}" y="282" width="32" height="40" rx="2" class="svg-load"/><line x1="${c - 8}" y1="292" x2="${c + 8}" y2="292" class="svg-load"/><line x1="${c - 8}" y1="302" x2="${c + 8}" y2="302" class="svg-load"/><line x1="${c - 8}" y1="312" x2="${c + 8}" y2="312" class="svg-load"/>`,
};

let I18N = {};
const S = {
  lang: localStorage.getItem('es_lang') || 'zh',
  devices: {}, tou_rate: '', counts: { active: 0, acked: 0 },
  metric: 'p', hours: 24, trendSel: new Set(FEEDERS),
  alarmFilter: 'active', energyView: 'day',
  tariff: {}, currency: 'CNY',
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
  document.getElementById('langSel').value = l;
  buildFeeders(); buildCards(); bindTrendDevs(); loadApiDoc();
  renderTou(); updateDiagram(); loadTab(curTab);
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

/* ---------- 接线图 ---------- */
function buildFeeders() {
  const g = document.getElementById('g-feeders');
  let svg = '';
  for (const id of FEEDERS) {
    const { x, color, icon } = CFG[id];
    svg += `
    <line x1="${x}" y1="160" x2="${x}" y2="172" class="wire"/>
    <rect id="brk-${id}" x="${x - 13}" y="172" width="26" height="16" rx="2" class="breaker"/>
    <line x1="${x}" y1="188" x2="${x}" y2="200" class="wire"/>
    <g id="mb-${id}" style="cursor:pointer">
      <rect x="${x - 62}" y="200" width="124" height="56" rx="6" class="meter-box" id="box-${id}"/>
      <circle id="dot-${id}" cx="${x - 50}" cy="212" r="4" class="dot"/>
      <text x="${x - 40}" y="217" class="svg-name" id="nm-${id}">${t('dev.' + id)}</text>
      <text x="${x}" y="240" text-anchor="middle" class="svg-val" fill="${color}" id="v-${id}-p">--</text>
      <text x="${x - 30}" y="252" text-anchor="middle" class="svg-sub" id="v-${id}-u">-- V</text>
      <text x="${x + 32}" y="252" text-anchor="middle" class="svg-sub" id="v-${id}-i">-- A</text>
    </g>
    <line x1="${x}" y1="256" x2="${x}" y2="282" class="wire"/>
    ${ICONS[icon](x)}
    <text x="${x}" y="346" text-anchor="middle" class="svg-load-tx">${t('dev.' + id)}</text>`;
  }
  g.innerHTML = svg;
  for (const id of FEEDERS) {
    document.getElementById('mb-' + id).onclick = () => {
      S.trendSel = new Set([id]); switchTab('trend');
    };
    document.getElementById('brk-' + id).onclick = () => {
      const on = !(S.devices[id]?.breaker ?? 1);
      api('/api/sim/cmd', { method: 'POST', body: JSON.stringify({ action: 'breaker', device: id, on }) });
    };
  }
}
function updateDiagram() {
  const d = S.devices;
  const upd = (id, keys) => {
    const st = d[id]; if (!st) return;
    keys.forEach(k => { const el = document.getElementById(`v-${id}-${k}`); if (el) el.textContent = k === 'pf' ? f2(st.pf) : (k === 'p' ? f1(st.p) : f1(st[k])); });
    const dot = document.getElementById('dot-' + id);
    if (dot) dot.classList.toggle('bad', !st._comm);
    const brk = document.getElementById('brk-' + id);
    if (brk) brk.classList.toggle('off', !st.breaker);
  };
  upd('meter-main', ['p', 'u', 'i', 'pf']);
  FEEDERS.forEach(id => upd(id, ['p', 'u', 'i']));
  document.getElementById('nm-meter-main').textContent = t('dev.meter-main');
}

/* ---------- 设备卡 ---------- */
function buildCards() {
  const wrap = document.getElementById('devCards');
  wrap.innerHTML = Object.keys(CFG).sort((a, b) => CFG[a].order - CFG[b].order).map(id => {
    const rows = CFG[id].water
      ? `<div class="row"><span>MPa</span><span id="c-${id}-pressure">--</span></div>
         <div class="row"><span>Hz</span><span id="c-${id}-f">--</span></div>
         <div class="row"><span>m³/h</span><span id="c-${id}-flow">--</span></div>
         <div class="row"><span>EPi kWh</span><span id="c-${id}-epi">--</span></div>`
      : `<div class="row"><span>U</span><span id="c-${id}-u">--</span></div>
         <div class="row"><span>I</span><span id="c-${id}-i">--</span></div>
         <div class="row"><span>PF</span><span id="c-${id}-pf">--</span></div>
         <div class="row"><span>EPi kWh</span><span id="c-${id}-epi">--</span></div>`;
    return `
    <div class="devcard" style="border-top:3px solid ${CFG[id].color}">
      <div class="name"><span>${t('dev.' + id)}</span><span class="dot" id="cdot-${id}"></span></div>
      <div class="p"><span id="c-${id}-p">--</span> <small>kW</small></div>
      ${rows}
    </div>`;
  }).join('');
}
function updateCards() {
  for (const id of Object.keys(CFG)) {
    const st = S.devices[id]; if (!st) continue;
    ['p', 'u', 'i', 'epi', 'pressure', 'f', 'flow'].forEach(k => { const el = document.getElementById(`c-${id}-${k}`); if (el) el.textContent = f2(st[k]); });
    const pf = document.getElementById(`c-${id}-pf`); if (pf) pf.textContent = f2(st.pf);
    const dot = document.getElementById(`cdot-${id}`); if (dot) dot.classList.toggle('bad', !st._comm);
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
function openSSE() {
  const es = new EventSource('/api/stream');
  es.onmessage = e => {
    const m = JSON.parse(e.data);
    if (m.type === 'tick') {
      S.devices = m.devices; S.counts = m.alarm_counts || S.counts;
      if (m.tou_rate) S.tou_rate = m.tou_rate;
      renderTou(); updateDiagram(); updateCards(); updateWater();
    } else if (m.type === 'alarm') {
      const a = m.alarm || {};
      if (m.event === 'triggered') {
        toast('🚨 ' + locMsg(a), 'danger');
        if (curTab === 'alarms') loadAlarms();
      } else if (m.event === 'closed' && curTab === 'alarms') loadAlarms();
    }
  };
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

/* ---------- 趋势 ---------- */
function bindTrendDevs() {
  document.getElementById('trendDevs').innerHTML = [...FEEDERS, 'ws-pump'].map(id =>
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
  drawChart(document.getElementById('trendCanvas'), series);
  document.getElementById('trendLegend').innerHTML = series.map(s =>
    `<span><span class="key" style="background:${s.color}"></span>${s.name}</span>`).join('');
}
function drawChart(canvas, series) {
  const dpr = window.devicePixelRatio || 1;
  if (!canvas.dataset.h) canvas.dataset.h = canvas.getAttribute('height');
  const W = canvas.clientWidth || 900, H = +canvas.dataset.h;
  canvas.width = W * dpr; canvas.height = H * dpr;
  const ctx = canvas.getContext('2d'); ctx.scale(dpr, dpr);
  ctx.clearRect(0, 0, W, H);
  const all = series.flatMap(s => s.data);
  if (!all.length) { ctx.fillStyle = '#94a3b8'; ctx.fillText(t('trend.nodata'), W / 2 - 30, H / 2); return; }
  const xs = all.map(p => p[0]);
  const x0 = Math.min(...xs), x1 = Math.max(...xs) || x0 + 1;
  let y0 = Math.min(...all.map(p => p[1])), y1 = Math.max(...all.map(p => p[1]));
  const pad = (y1 - y0) * 0.1 || 1; y0 -= pad; y1 += pad;
  const L = 56, R = 12, T = 12, B = 26;
  const px = v => L + (v - x0) / (x1 - x0) * (W - L - R);
  const py = v => H - B - (v - y0) / (y1 - y0) * (H - T - B);
  ctx.strokeStyle = '#24304d'; ctx.fillStyle = '#94a3b8'; ctx.font = '11px sans-serif'; ctx.lineWidth = 1;
  for (let i = 0; i <= 4; i++) {
    const v = y0 + (y1 - y0) * i / 4, y = py(v);
    ctx.beginPath(); ctx.moveTo(L, y); ctx.lineTo(W - R, y); ctx.stroke();
    ctx.fillText(v.toFixed(Math.abs(y1 - y0) < 5 ? 2 : 0), 6, y + 4);
  }
  for (let i = 0; i <= 4; i++) {
    const v = x0 + (x1 - x0) * i / 4, x = px(v);
    ctx.fillText(fdate(v).slice(5), x - 22, H - 8);
  }
  for (const s of series) {
    if (!s.data.length) continue;
    ctx.strokeStyle = s.color; ctx.lineWidth = 1.8; ctx.beginPath();
    s.data.forEach((p, i) => i ? ctx.lineTo(px(p[0]), py(p[1])) : ctx.moveTo(px(p[0]), py(p[1])));
    ctx.stroke();
  }
}

/* ---------- 供水实验台 ---------- */
function updateWater() {
  const st = S.devices['ws-pump']; if (!st) return;
  const set = (id, v) => { const el = document.getElementById(id); if (el) el.textContent = v; };
  set('wsPressureVal', f2(st.pressure)); set('wsSpVal', 'SP ' + f2(st.sp));
  set('wsFreqVal', f1(st.f) + ' Hz'); set('wsFlowVal', f1(st.flow)); set('wsPowerVal', f1(st.p));
  const dot = document.getElementById('wsDot'); if (dot) dot.classList.toggle('bad', !st._comm);
  const imp = document.getElementById('wsImpeller');
  if (imp && st.f > 5) imp.setAttribute('transform', `rotate(${(Date.now() / (3000 / st.f * 50)) % 360} 330 205)`);
  const lvl = document.getElementById('wsTankLevel');
  if (lvl) { const h = 82 - (st.f - 20) * 1.2; lvl.setAttribute('y', 222 - Math.max(20, Math.min(82, h))); lvl.setAttribute('height', Math.max(20, Math.min(82, h))); }
}
async function loadWaterTrend() {
  const d = await api('/api/history?devices=ws-pump&metric=pressure&hours=1&max=360');
  drawChart(document.getElementById('wsCanvasP'), [
    { name: 'MPa', color: '#38bdf8', data: d.series['ws-pump'] || [] },
  ]);
  const d2 = await api('/api/history?devices=ws-pump&metric=f&hours=1&max=360');
  drawChart(document.getElementById('wsCanvasF'), [
    { name: 'Hz', color: '#f59e0b', data: d2.series['ws-pump'] || [] },
  ]);
}

/* ---------- 能耗报表 ---------- */
async function loadEnergy() {
  const dateEl = document.getElementById('energyDate');
  if (!dateEl.value) dateEl.valueAsDate = new Date();
  const date = dateEl.value;
  const host = document.getElementById('energyTable');
  const canvas = document.getElementById('energyCanvas');
  if (S.energyView === 'day') {
    const d = await api(`/api/report/day?date=${date}`);
    document.getElementById('energyTariff').textContent =
      `${t('energy.peak')}${d.tariff.peak} / ${t('energy.flat')}${d.tariff.flat} / ${t('energy.valley')}${d.tariff.valley} ${d.currency === 'CNY' ? '¥/kWh' : '$/kWh'}`;
    const rows = d.devices.map(v => `<tr>
      <td>${t('dev.' + v.id, null) || v.name}</td><td>${v.total_kwh}</td><td>${v.peak_kwh}</td>
      <td>${v.flat_kwh}</td><td>${v.valley_kwh}</td><td>${v.cost}</td></tr>`);
    host.innerHTML = `<table><tr><th>${t('energy.device')}</th><th>kWh ${t('energy.total')}</th>
      <th>${t('energy.peak')}</th><th>${t('energy.flat')}</th><th>${t('energy.valley')}</th><th>${t('energy.cost')}</th></tr>
      ${rows.join('')}<tr style="font-weight:700"><td>${t('energy.totals')}</td><td>${d.totals.total_kwh}</td>
      <td>${d.totals.peak_kwh}</td><td>${d.totals.flat_kwh}</td><td>${d.totals.valley_kwh}</td><td>${d.totals.cost}</td></tr></table>`;
    drawStackedBar(canvas, d.devices.map(v => ({ name: t('dev.' + v.id) || v.name, peak: v.peak_kwh, flat: v.flat_kwh, valley: v.valley_kwh })));
  } else {
    const month = date.slice(0, 7);
    const d = await api(`/api/report/month?month=${month}`);
    const rows = d.days.map(v => `<tr><td>${v.date}</td><td>${v.total_kwh}</td><td>${v.cost}</td></tr>`).join('');
    host.innerHTML = `<table><tr><th>${t('energy.date')}</th><th>kWh</th><th>${t('energy.cost')}</th></tr>${rows}</table>`;
    drawChart(canvas, [{ name: 'kWh', color: '#22d3ee', data: d.days.map(v => [new Date(v.date).getTime(), v.total_kwh]) }]);
  }
}
function drawStackedBar(canvas, items) {
  const dpr = window.devicePixelRatio || 1;
  if (!canvas.dataset.h) canvas.dataset.h = canvas.getAttribute('height');
  const W = canvas.clientWidth || 900, H = +canvas.dataset.h;
  canvas.width = W * dpr; canvas.height = H * dpr;
  const ctx = canvas.getContext('2d'); ctx.scale(dpr, dpr); ctx.clearRect(0, 0, W, H);
  if (!items.length) return;
  const max = Math.max(...items.map(i => i.peak + i.flat + i.valley)) || 1;
  const L = 40, R = 10, T = 10, B = 26;
  const bw = Math.min(90, (W - L - R) / items.length * 0.55);
  const colors = { peak: '#f59e0b', flat: '#60a5fa', valley: '#34d399' };
  ctx.font = '11px sans-serif'; ctx.fillStyle = '#94a3b8';
  items.forEach((it, i) => {
    const cx = L + (i + 0.5) * (W - L - R) / items.length;
    let y = H - B;
    for (const k of ['peak', 'flat', 'valley']) {
      const h = it[k] / max * (H - T - B);
      ctx.fillStyle = colors[k]; ctx.fillRect(cx - bw / 2, y - h, bw, h); y -= h;
    }
    ctx.fillStyle = '#94a3b8';
    ctx.fillText(it.name.slice(0, 8), cx - 24, H - 8);
    ctx.fillText((it.peak + it.flat + it.valley).toFixed(1), cx - 14, y - 4);
  });
}

/* ---------- 告警 ---------- */
async function loadAlarms() {
  const d = await api(`/api/alarms?status=${S.alarmFilter}&limit=100`);
  const host = document.getElementById('alarmTable');
  if (!d.alarms.length) { host.innerHTML = `<p class="muted">${t('alarm.empty')}</p>`; return; }
  host.innerHTML = `<table><tr><th>${t('alarm.triggered_at')}</th><th>${t('alarm.sev')}</th><th>${t('dev.title')}</th>
    <th>${t('alarm.value')}</th><th>${t('common.status')}</th><th></th></tr>` +
    d.alarms.map(a => `<tr>
      <td>${fdate(new Date(a.triggered_at).getTime())}</td>
      <td><span class="sev ${a.severity}">${t('alarm.s' + a.severity)}</span></td>
      <td>${esc(devName(a.device_id))}</td>
      <td>${esc(locMsg(a))}${a.metric !== 'comm' ? ` <b>(${f1(a.value)})</b>` : ''}</td>
      <td><span class="st ${a.status}">${t('alarm.' + a.status)}</span></td>
      <td>${a.status === 'active' ? `<button class="btn sm" onclick="ackAlarm(${a.id})">${t('alarm.ack')}</button> ` : ''}
          ${a.status !== 'closed' ? `<button class="btn sm" onclick="closeAlarm(${a.id})">${t('alarm.close')}</button>
          <button class="btn sm" onclick="toWo(${a.id}, ${JSON.stringify(locMsg(a)).replaceAll('"', '&quot;')})">${t('alarm.towo')}</button>` : ''}</td>
    </tr>`).join('') + '</table>';
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
  if (!d.workorders.length) { host.innerHTML = `<p class="muted">${t('wo.empty')}</p>`; return; }
  host.innerHTML = `<table><tr><th>#</th><th>${t('wo.title')}</th><th>${t('common.status')}</th>
    <th>${t('wo.assignee')}</th><th>${t('wo.created')}</th><th>${t('wo.note')}</th><th></th></tr>` +
    d.workorders.map(w => `<tr>
      <td>${w.id}</td><td>${esc(w.title)}${w.alarm_id ? ` <span class="sev ${w.severity || 'info'}">A#${w.alarm_id}</span>` : ''}</td>
      <td><span class="st ${w.status}">${t('wo.st' + w.status)}</span></td>
      <td>${esc(w.assignee) || '--'}</td>
      <td>${fdate(new Date(w.created_at).getTime())}</td>
      <td><div class="notes">${(w.notes || []).map(n => `<div>${n.at} ${esc(n.author)}: ${esc(n.note)}</div>`).join('')}</div>
        <input id="wn-${w.id}" size="14"> <button class="btn sm" onclick="addNote(${w.id})">${t('wo.addnote')}</button></td>
      <td>${w.status === 'open' ? `<button class="btn sm" onclick="woSt(${w.id},'processing')">${t('wo.set_processing')}</button> ` : ''}
          ${w.status !== 'closed' ? `<button class="btn sm danger" onclick="woSt(${w.id},'closed')">${t('wo.close_wo')}</button>` : ''}</td>
    </tr>`).join('') + '</table>';
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
  document.getElementById('devTable').innerHTML = `<table>
    <tr><th>${t('dev.name')}</th><th>Kind</th><th>Driver</th><th>${t('dev.comm')}</th>
    <th>${t('dev.last_ts')}</th><th>P kW</th><th>EPi kWh</th><th>Modbus</th></tr>` +
    d.devices.map(v => `<tr>
      <td>${devName(v.id, v.name)} <span class="muted">${esc(v.id)}</span></td>
      <td>${v.kind}</td><td><code>${v.driver}</code></td>
      <td><span class="dot ${v.comm ? '' : 'bad'}"></span></td>
      <td>${v.last_ts ? ftime(v.last_ts) : '--'}</td>
      <td>${f1(v.p)}</td><td>${f1(v.epi)}</td>
      <td class="muted">${(v.metrics || []).map(m => `${m.metric}:0x${m.reg ?? '--'}`).join(' ')}</td>
    </tr>`).join('') + '</table>';
  const sel = document.getElementById('ruleDev');
  sel.innerHTML = d.devices.map(v => `<option value="${esc(v.id)}">${devName(v.id, v.name)}</option>`).join('');
}
async function loadRules() {
  const d = await api('/api/rules');
  document.getElementById('ruleTable').innerHTML = `<table>
    <tr><th>#</th><th>${t('dev.name')}</th><th>${t('trend.metric')}</th><th>${t('dev.cond')}</th>
    <th>${t('dev.duration')}s</th><th>${t('dev.hysteresis')}</th><th>${t('dev.severity')}</th><th></th></tr>` +
    d.rules.map(r => `<tr>
      <td>${r.id}</td><td>${t('dev.' + r.device_id)}</td><td>${r.metric}</td>
      <td>${r.op === 'gt' ? '&gt;' : '&lt;'} ${r.threshold}</td>
      <td>${r.duration_s}</td><td>${r.hysteresis}</td>
      <td><span class="sev ${r.severity}">${r.severity}</span></td>
      <td><button class="btn sm" onclick="toggleRule(${r.id})">${r.enabled ? '✅' : '⛔'}</button>
      <button class="btn sm danger" onclick="delRule(${r.id})">✕</button></td>
    </tr>`).join('') + '</table>';
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
  document.getElementById('apiDoc').textContent =
`# ${t('api.doc.latest')}
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
}

/* ---------- 绑定 ---------- */
function bind() {
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
        severity: document.getElementById('ruleSev').value,
      }),
    });
    loadRules(); toast(t('common.saved'));
  };
  document.getElementById('btnOverIm').onclick = () => api('/api/sim/cmd', { method: 'POST', body: JSON.stringify({ action: 'overload', device: 'feeder-im', kw: 40, secs: 120 }) }).then(() => toast('⚡ +40kW → feeder-im', 'warn'));
  document.getElementById('btnOverAc').onclick = () => api('/api/sim/cmd', { method: 'POST', body: JSON.stringify({ action: 'overload', device: 'feeder-ac', kw: 20, secs: 120 }) }).then(() => toast('⚡ +20kW → feeder-ac', 'warn'));
  document.getElementById('btnComm').onclick = () => api('/api/sim/cmd', { method: 'POST', body: JSON.stringify({ action: 'comm_fault', device: 'meter-main', secs: 45 }) }).then(() => toast('📴 comm fault → meter-main', 'warn'));
  document.getElementById('btnClear').onclick = () => api('/api/sim/cmd', { method: 'POST', body: JSON.stringify({ action: 'clear' }) }).then(() => toast('✅ cleared'));
  document.getElementById('wsApply').onclick = () => api('/api/sim/cmd', { method: 'POST', body: JSON.stringify({ action: 'water_sp', sp: +document.getElementById('wsSpInput').value }) }).then(() => toast('SP → ' + document.getElementById('wsSpInput').value + ' MPa'));
  document.getElementById('wsDemand').onclick = () => api('/api/sim/cmd', { method: 'POST', body: JSON.stringify({ action: 'water_demand', pct: 1.0, secs: 90 }) }).then(() => toast('🚰 +100% demand 90s', 'warn'));
  document.getElementById('wsNormal').onclick = () => api('/api/sim/cmd', { method: 'POST', body: JSON.stringify({ action: 'water_demand', pct: 0, secs: 0.1 }) }).then(() => toast('✅'));
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
  await loadLang(S.lang);
  openSSE();
  api('/api/state').then(s => { S.tariff = s.tariff; S.currency = s.currency; }).catch(() => {});
  setInterval(() => { if (curTab === 'trend') loadTrend(); }, 60_000);
  setInterval(() => { if (curTab === 'water') loadWaterTrend(); }, 30_000);
})();
