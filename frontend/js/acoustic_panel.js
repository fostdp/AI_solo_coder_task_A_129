const API = (window.location.protocol === 'file:')
  ? 'http://127.0.0.1:8080/api'
  : '/api';

export class AcousticPanel {
  constructor(drum3D) {
    this.drum3D = drum3D;
    this.state = {
      drums: [],
      currentDrum: null,
      currentDrumId: null,
      castingResult: null,
      acousticResult: null,
      modes: [],
      shrinkageMap: [],
      coolingRateMap: [],
      defects: [],
      soundField: [],
      wallThickness: [],
      alarms: [],
      spectrum: [],
      currentDrumAlloy: null,
    };
    this.ws = null;
  }

  async apiGet(path) {
    try { const r = await fetch(`${API}${path}`); return await r.json(); }
    catch (e) { return { success: false, error: String(e) }; }
  }

  async apiPost(path, body) {
    try {
      const r = await fetch(`${API}${path}`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });
      return await r.json();
    } catch (e) { return { success: false, error: String(e) }; }
  }

  drawGauge(id, value, max = 100, label = '') {
    const c = document.getElementById(id); if (!c) return;
    const dpr = window.devicePixelRatio || 1;
    c.width = c.clientWidth * dpr; c.height = c.clientHeight * dpr;
    const ctx = c.getContext('2d'); ctx.scale(dpr, dpr);
    const w = c.clientWidth, h = c.clientHeight;
    ctx.clearRect(0, 0, w, h);
    const cx = w / 2, cy = h * 0.9, r = Math.min(w, h * 1.8) * 0.4;
    ctx.lineWidth = 14; ctx.lineCap = 'round';
    ctx.beginPath(); ctx.arc(cx, cy, r, Math.PI, 0); ctx.strokeStyle = '#334155'; ctx.stroke();
    const ang = Math.PI + (value / max) * Math.PI;
    const grd = ctx.createLinearGradient(cx - r, 0, cx + r, 0);
    grd.addColorStop(0, '#22c55e'); grd.addColorStop(0.5, '#eab308'); grd.addColorStop(1, '#ef4444');
    ctx.strokeStyle = grd; ctx.beginPath(); ctx.arc(cx, cy, r, Math.PI, ang); ctx.stroke();
    const px = cx + Math.cos(ang) * (r - 5), py = cy + Math.sin(ang) * (r - 5);
    ctx.strokeStyle = '#f8fafc'; ctx.lineWidth = 3;
    ctx.beginPath(); ctx.moveTo(cx, cy); ctx.lineTo(px, py); ctx.stroke();
    ctx.beginPath(); ctx.arc(cx, cy, 7, 0, Math.PI * 2); ctx.fillStyle = '#e2e8f0'; ctx.fill();
    ctx.fillStyle = '#f1f5f9'; ctx.font = 'bold 32px sans-serif'; ctx.textAlign = 'center';
    ctx.fillText(value.toFixed(0), cx, cy - r * 0.45);
    ctx.font = '11px sans-serif'; ctx.fillStyle = '#94a3b8';
    ctx.fillText(label, cx, cy - r * 0.45 + 20);
  }

  shade(hex, pct) {
    const f = parseInt(hex.slice(1), 16);
    const t = pct < 0 ? 0 : 255, p = Math.abs(pct);
    const R = f >> 16, G = (f >> 8) & 0xff, B = f & 0xff;
    return '#' + (0x1000000 + (Math.round((t - R) * p) + R) * 0x10000 + (Math.round((t - G) * p) + G) * 0x100 + (Math.round((t - B) * p) + B)).toString(16).slice(1);
  }

  drawAlloyChart() {
    const c = document.getElementById('alloy-chart'); if (!c) return;
    const dpr = window.devicePixelRatio || 1;
    c.width = c.clientWidth * dpr; c.height = c.clientHeight * dpr;
    const ctx = c.getContext('2d'); ctx.scale(dpr, dpr);
    const w = c.clientWidth, h = c.clientHeight;
    ctx.clearRect(0, 0, w, h);
    const a = this.state.currentDrumAlloy || { copper_pct: 78, tin_pct: 18, lead_pct: 3, zinc_pct: 0.5, other_impurities_pct: 0.5 };
    const items = [
      { k: 'Cu', v: a.copper_pct, c: '#b45309' },
      { k: 'Sn', v: a.tin_pct, c: '#a16207' },
      { k: 'Pb', v: a.lead_pct, c: '#65a30d' },
      { k: 'Zn', v: a.zinc_pct, c: '#0891b2' },
      { k: '杂', v: a.other_impurities_pct, c: '#7c3aed' },
    ];
    const total = items.reduce((s, i) => s + i.v, 0);
    let x = 0;
    items.forEach(it => {
      const bw = (it.v / total) * w;
      const grd = ctx.createLinearGradient(0, 0, 0, h * 0.6);
      grd.addColorStop(0, it.c); grd.addColorStop(1, this.shade(it.c, -0.3));
      ctx.fillStyle = grd; ctx.fillRect(x, 0, bw, h * 0.6);
      ctx.fillStyle = '#f1f5f9'; ctx.font = 'bold 11px sans-serif'; ctx.textAlign = 'center';
      ctx.fillText(`${it.v.toFixed(1)}%`, x + bw / 2, h * 0.35);
      ctx.font = '10px sans-serif'; ctx.fillStyle = '#94a3b8';
      ctx.fillText(it.k, x + bw / 2, h * 0.6 + 14);
      x += bw;
    });
  }

  drawSpectrumChart() {
    const c = document.getElementById('spectrum-chart'); if (!c) return;
    const dpr = window.devicePixelRatio || 1;
    c.width = c.clientWidth * dpr; c.height = c.clientHeight * dpr;
    const ctx = c.getContext('2d'); ctx.scale(dpr, dpr);
    const w = c.clientWidth, h = c.clientHeight;
    ctx.clearRect(0, 0, w, h);
    const d = this.state.spectrum;
    if (!d || !d.length) {
      ctx.fillStyle = '#475569'; ctx.font = '12px sans-serif'; ctx.textAlign = 'center';
      ctx.fillText('暂无频谱数据', w / 2, h / 2);
      return;
    }
    ctx.strokeStyle = '#334155'; ctx.lineWidth = 1;
    ctx.beginPath(); ctx.moveTo(40, 10); ctx.lineTo(40, h - 25); ctx.lineTo(w - 10, h - 25); ctx.stroke();
    for (let db = -80; db <= 20; db += 20) {
      const y = h - 25 - ((db + 80) / 100) * (h - 35);
      ctx.fillStyle = '#64748b'; ctx.font = '9px sans-serif'; ctx.textAlign = 'right';
      ctx.fillText(`${db}dB`, 35, y + 3);
      ctx.strokeStyle = '#1e293b';
      ctx.beginPath(); ctx.moveTo(42, y); ctx.lineTo(w - 10, y); ctx.stroke();
    }
    const maxF = d[d.length - 1].frequency_hz;
    for (let f = 0; f <= maxF; f += 500) {
      const x = 42 + (f / maxF) * (w - 52);
      ctx.fillStyle = '#64748b'; ctx.font = '9px sans-serif'; ctx.textAlign = 'center';
      ctx.fillText(`${f}`, x, h - 12);
    }
    ctx.beginPath();
    d.forEach((pt, i) => {
      const x = 42 + (pt.frequency_hz / maxF) * (w - 52);
      const y = h - 25 - ((pt.amplitude_db + 80) / 100) * (h - 35);
      i ? ctx.lineTo(x, y) : ctx.moveTo(x, y);
    });
    ctx.lineTo(w - 10, h - 25); ctx.lineTo(42, h - 25); ctx.closePath();
    const g = ctx.createLinearGradient(0, 10, 0, h - 25);
    g.addColorStop(0, 'rgba(234,179,8,0.6)'); g.addColorStop(1, 'rgba(234,179,8,0.02)');
    ctx.fillStyle = g; ctx.fill();
    ctx.beginPath();
    d.forEach((pt, i) => {
      const x = 42 + (pt.frequency_hz / maxF) * (w - 52);
      const y = h - 25 - ((pt.amplitude_db + 80) / 100) * (h - 35);
      i ? ctx.lineTo(x, y) : ctx.moveTo(x, y);
    });
    ctx.strokeStyle = '#eab308'; ctx.lineWidth = 1.5; ctx.stroke();
  }

  drawFrequencyChart() {
    const c = document.getElementById('frequency-chart'); if (!c) return;
    const dpr = window.devicePixelRatio || 1;
    c.width = c.clientWidth * dpr; c.height = c.clientHeight * dpr;
    const ctx = c.getContext('2d'); ctx.scale(dpr, dpr);
    const w = c.clientWidth, h = c.clientHeight;
    ctx.clearRect(0, 0, w, h);
    ctx.fillStyle = '#475569'; ctx.font = 'bold 10px sans-serif'; ctx.textAlign = 'center';
    ctx.fillText('固有频率 vs 参考音高 (Hz)', w / 2, 14);
    const freqs = this.state.acousticResult?.resonance_frequencies_hz || this.state.modes.map(m => m.frequency_hz) || [];
    const std = [{ f: 523.25, n: 'C5' }, { f: 659.25, n: 'E5' }, { f: 783.99, n: 'G5' }, { f: 1046.5, n: 'C6' }, { f: 1318.51, n: 'E6' }];
    if (!freqs.length) {
      ctx.fillStyle = '#475569'; ctx.font = '12px sans-serif';
      ctx.fillText('请先运行声学分析', w / 2, h / 2);
      return;
    }
    const maxF = Math.max(...freqs, ...std.map(s => s.f)) * 1.1;
    ctx.strokeStyle = '#334155';
    ctx.beginPath(); ctx.moveTo(50, h - 30); ctx.lineTo(w - 20, h - 30); ctx.stroke();
    std.forEach(s => {
      const x = 50 + (s.f / maxF) * (w - 70);
      ctx.strokeStyle = '#1e40af'; ctx.setLineDash([2, 3]); ctx.globalAlpha = 0.4;
      ctx.beginPath(); ctx.moveTo(x, 30); ctx.lineTo(x, h - 30); ctx.stroke();
      ctx.setLineDash([]); ctx.globalAlpha = 1;
      ctx.fillStyle = '#60a5fa'; ctx.font = '9px sans-serif'; ctx.fillText(s.n, x, h - 15);
    });
    freqs.forEach((f, i) => {
      const x = 50 + (f / maxF) * (w - 70);
      const bh = 25 + (1 - i / freqs.length) * (h - 80);
      const y = h - 30 - bh;
      const grd = ctx.createLinearGradient(0, y, 0, h - 30);
      grd.addColorStop(0, '#8b5cf6'); grd.addColorStop(1, '#ec4899');
      ctx.fillStyle = grd;
      const rr = 3;
      ctx.beginPath();
      ctx.moveTo(x - 6 + rr, y);
      ctx.arcTo(x + 6, y, x + 6, y + bh, rr);
      ctx.arcTo(x + 6, y + bh, x - 6, y + bh, rr);
      ctx.arcTo(x - 6, y + bh, x - 6, y, rr);
      ctx.arcTo(x - 6, y, x + 6, y, rr);
      ctx.closePath(); ctx.fill();
      ctx.fillStyle = '#f1f5f9'; ctx.font = 'bold 9px sans-serif';
      ctx.fillText(`${f.toFixed(0)}`, x, y - 4);
      ctx.fillStyle = '#94a3b8'; ctx.font = '8px sans-serif';
      ctx.fillText(`M${i + 1}`, x, h - 35);
    });
  }

  drawThicknessHeatmap() {
    const c = document.getElementById('thickness-heatmap'); if (!c) return;
    const dpr = window.devicePixelRatio || 1;
    c.width = c.clientWidth * dpr; c.height = c.clientHeight * dpr;
    const ctx = c.getContext('2d'); ctx.scale(dpr, dpr);
    const w = c.clientWidth, h = c.clientHeight;
    ctx.clearRect(0, 0, w, h);
    const data = this.state.shrinkageMap.length ? this.state.shrinkageMap :
      this.state.wallThickness.map(t => [t.x_frac, t.y_frac, t.thickness_mm]);
    ctx.fillStyle = '#475569'; ctx.font = '11px sans-serif'; ctx.textAlign = 'center';
    if (!data.length) { ctx.fillText('暂无数据', w / 2, h / 2); return; }
    const res = Math.ceil(Math.sqrt(data.length));
    const cellW = w / res, cellH = h / res;
    const vs = data.map(d => d[2]);
    const mn = Math.min(...vs), mx = Math.max(...vs);
    const rng = mx - mn || 1;
    data.forEach(([x, y, v]) => {
      ctx.fillStyle = this.colormap((v - mn) / rng, this.state.shrinkageMap.length ? 'hot' : 'viridis');
      ctx.fillRect(x * w - cellW / 2, y * h - cellH / 2, cellW + 1, cellH + 1);
    });
    ctx.strokeStyle = 'rgba(234,179,8,0.6)'; ctx.lineWidth = 1.5;
    ctx.beginPath();
    ctx.ellipse(w / 2, h / 2, w * 0.46, h * 0.46, 0, 0, Math.PI * 2);
    ctx.stroke();
  }

  colormap(t, kind = 'viridis') {
    t = Math.max(0, Math.min(1, t));
    if (kind === 'hot') {
      const r = Math.min(1, t * 2.5), g = Math.max(0, Math.min(1, (t - 0.2) * 2.5));
      const b = Math.max(0, Math.min(1, (t - 0.7) * 3));
      return `rgb(${r * 255 | 0},${g * 255 | 0},${b * 255 | 0})`;
    }
    const stops = [[68, 1, 84], [59, 82, 139], [33, 145, 140], [94, 201, 98], [253, 231, 37]];
    const s = t * (stops.length - 1), i = Math.floor(s), f = s - i;
    const a = stops[i], b = stops[Math.min(stops.length - 1, i + 1)];
    return `rgb(${a[0] + (b[0] - a[0]) * f | 0},${a[1] + (b[1] - a[1]) * f | 0},${a[2] + (b[2] - a[2]) * f | 0})`;
  }

  buildLegend(container, title, colors, labels) {
    container.innerHTML = `<div class="text-slate-300 font-semibold mb-1">${title}</div>`;
    for (let i = colors.length - 1; i >= 0; i--) {
      container.innerHTML += `<div class="flex items-center gap-2"><div class="w-4 h-3 rounded" style="background:${colors[i]}"></div><span class="text-slate-400">${labels[i]}</span></div>`;
    }
  }

  switchView(viewName) {
    this.drum3D.switchView(viewName);
    const legend = document.getElementById('legend-bars');
    legend.innerHTML = '';
    switch (viewName) {
      case 'modes':
        this.buildLegend(legend, '位移幅度', ['#3b82f6', '#8b5cf6', '#ec4899', '#ef4444'], ['0', '0.33', '0.66', '1']);
        break;
      case 'soundfield':
        this.buildLegend(legend, 'SPL (dB)', ['#1e3a8a', '#3b82f6', '#10b981', '#eab308', '#ef4444'], ['40', '60', '80', '100', '120']);
        break;
      case 'defects':
        this.buildLegend(legend, '缺陷严重度', ['#22c55e', '#eab308', '#f97316', '#ef4444', '#991b1b'], ['低', '中', '中高', '高', '致命']);
        break;
    }
    const ov = document.getElementById('overlay-text');
    const viewLabels = {
      '3d': '铜鼓三维几何模型（可拖拽旋转/滚轮缩放）',
      'modes': `模态振型动画：${this.state.modes[this.drum3D.state.currentModeIndex]?.node_pattern || '请先运行声学分析'}`,
      'soundfield': '远场声辐射声压云图（SPL dB）',
      'defects': `铸造缺陷分布预测：共 ${this.state.defects.length} 处`,
    };
    ov.textContent = viewLabels[viewName] || '';
  }

  refreshAllCharts() {
    this.drawAlloyChart();
    this.drawSpectrumChart();
    this.drawFrequencyChart();
    this.drawThicknessHeatmap();
    const qc = this.state.castingResult?.quality_score ?? 0;
    const aq = this.state.acousticResult?.sound_quality_metric ?? 0;
    const score = this.state.castingResult || this.state.acousticResult ? ((qc || 0.6) + (aq || 0.6)) / 2 * 100 : 70;
    this.drawGauge('quality-gauge', score, 100, '综合品质分');
    document.getElementById('cast-quality').textContent = this.state.castingResult ? (qc * 100).toFixed(0) : '--';
    document.getElementById('acoustic-quality').textContent = this.state.acousticResult ? (aq * 100).toFixed(0) : '--';
    document.getElementById('defect-count').textContent = this.state.defects.length || '0';
    const rl = document.getElementById('risk-level');
    rl.textContent = this.state.castingResult?.overall_risk || '--';
    rl.className = 'text-xl font-bold ' + ({
      CRITICAL: 'text-red-400', HIGH: 'text-orange-400', MEDIUM: 'text-yellow-400', LOW: 'text-green-400'
    }[this.state.castingResult?.overall_risk] || 'text-slate-200');
  }

  populateModeSelect() {
    const sel = document.getElementById('mode-select');
    sel.innerHTML = this.state.modes.length
      ? this.state.modes.map(m => `<option value="${m.mode_order - 1}">M${m.mode_order}: ${m.frequency_hz.toFixed(1)}Hz ${m.node_pattern}</option>`).join('')
      : '<option>暂无模态数据</option>';
  }

  async loadDrums() {
    const res = await this.apiGet('/drums');
    if (res.success && res.data) {
      this.state.drums = res.data;
      const sel = document.getElementById('drum-select');
      sel.innerHTML = res.data.map(d => `<option value="${d.drum_id}">${d.name} [${d.ethnic_group}]</option>`).join('');
      if (res.data.length) {
        sel.value = res.data[0].drum_id;
        this.selectDrum(res.data[0].drum_id);
      }
    }
    document.getElementById('last-update').textContent = '已连接 ' + new Date().toLocaleTimeString();
  }

  async selectDrum(id) {
    const drum = this.state.drums.find(d => d.drum_id === id);
    if (!drum) return;
    this.state.currentDrum = drum;
    this.state.currentDrumId = id;
    document.getElementById('dim-diameter').textContent = drum.diameter_cm.toFixed(1) + 'cm';
    document.getElementById('dim-height').textContent = drum.height_cm.toFixed(1) + 'cm';
    document.getElementById('dim-mass').textContent = drum.mass_kg.toFixed(1) + 'kg';
    this.drum3D.buildBronzeDrum(drum.diameter_cm, drum.height_cm);
    this.state.shrinkageMap = []; this.state.defects = []; this.state.castingResult = null;
    this.state.acousticResult = null; this.state.modes = []; this.state.soundField = [];
    this.state.spectrum = []; this.state.wallThickness = [];
    this.drum3D.setDefects([]);
    this.drum3D.setSoundField([]);
    this.refreshAllCharts(); this.populateModeSelect();

    const readings = await this.apiGet(`/sensor/readings/${id}`);
    if (readings.success && readings.data?.[0]) this.applySensorReading(readings.data[0]);

    const casting = await this.apiGet(`/casting/${id}`);
    if (casting.success && casting.data) this.applyCastingResult(casting.data);

    const acoustic = await this.apiGet(`/acoustics/${id}`);
    if (acoustic.success && acoustic.data) this.applyAcousticResult(acoustic.data);

    const alarms = await this.apiGet(`/alarms/${id}`);
    if (alarms.success && alarms.data) { this.state.alarms = alarms.data; this.renderAlarms(); }
  }

  applySensorReading(r) {
    this.state.currentDrumAlloy = r.alloy;
    this.state.spectrum = r.tap_spectrum;
    this.state.wallThickness = r.wall_thickness;
    this.refreshAllCharts();
  }

  applyCastingResult(r) {
    this.state.castingResult = r;
    this.state.shrinkageMap = r.shrinkage_risk_map || [];
    this.state.defects = r.defects || [];
    this.drum3D.setDefects(this.state.defects);
    if (this.state.defects.length) this.drum3D.buildDefectMarkers();
    this.state.currentDrumAlloy = r.alloy;
    this.refreshAllCharts();
  }

  applyAcousticResult(r) {
    this.state.acousticResult = r;
    this.state.modes = r.vibration_modes || [];
    this.state.soundField = r.sound_field || [];
    this.drum3D.setSoundField(this.state.soundField);
    if (this.state.soundField.length) this.drum3D.buildSoundField();
    if (this.state.modes.length) {
      this.drum3D.buildModeDisplacementTextures(this.state.modes, this.drum3D.radius);
    }
    this.populateModeSelect();
    this.refreshAllCharts();
  }

  renderAlarms() {
    const list = document.getElementById('alarm-list');
    document.getElementById('alarm-count').textContent = this.state.alarms.length;
    if (!this.state.alarms.length) {
      list.innerHTML = `<div class="text-slate-500 text-center py-8">暂无告警</div>`;
      return;
    }
    const cols = { Info: 'bg-blue-500/20 border-blue-500/40 text-blue-200', Warning: 'bg-yellow-500/20 border-yellow-500/40 text-yellow-200', Critical: 'bg-orange-500/20 border-orange-500/40 text-orange-200', Fatal: 'bg-red-600/30 border-red-500/60 text-red-200' };
    list.innerHTML = this.state.alarms.slice(0, 20).map(a => `
      <div class="border rounded-lg px-3 py-2 ${cols[a.severity] || cols.Info}">
        <div class="flex justify-between items-center gap-2 mb-1">
          <span class="font-semibold truncate">${a.alarm_type}</span>
          <span class="text-[10px] opacity-75">${new Date(a.timestamp).toLocaleString()}</span>
        </div>
        <div class="text-[11px] leading-snug opacity-90">${a.message}</div>
      </div>
    `).join('');
  }

  pushAlarm(a) {
    this.state.alarms.unshift(a);
    if (this.state.alarms.length > 200) this.state.alarms.length = 200;
    this.renderAlarms();
  }

  mockReading(drumId, fault = false) {
    const ref = [523.25, 659.25, 783.99, 1046.50, 1318.51];
    if (fault) ref[0] += 8;
    const spectrum = [];
    for (let f = 50; f <= 3000; f += (3000 - 50) / 256) {
      let amp = -60 - Math.random() * 5;
      ref.forEach((base, i) => {
        const d = Math.abs(f - base), w = 3 + 2 * i;
        if (d < 30) amp = Math.max(amp, 12 - 3.5 * i - (d / w) ** 2 * 6);
      });
      spectrum.push({ frequency_hz: +f.toFixed(2), amplitude_db: +Math.max(-80, amp + (Math.random() - 0.5) * 3).toFixed(2) });
    }
    const zones = ['鼓心/太阳纹区', '主晕圈/羽人纹区', '鼓面外圈/立蛙区', '鼓腰/胴部', '鼓足/底部边缘', '耳部/纹饰区'];
    const thick = [];
    for (let i = 0; i < 12; i++) {
      const a = i * Math.PI * 2 / 12;
      let t = 5 + Math.sin(a * 3) * 0.8 + (Math.random() - 0.5) * 0.6;
      if (fault && i === 4) t *= 0.6;
      thick.push({ zone: zones[i % zones.length], x_frac: +(0.5 + 0.4 * Math.cos(a)).toFixed(4), y_frac: +(0.5 + 0.4 * Math.sin(a)).toFixed(4), thickness_mm: +t.toFixed(3) });
    }
    return {
      reading_id: '', drum_id: drumId, timestamp: '',
      alloy: { copper_pct: fault ? 82 : 77.8, tin_pct: fault ? 14 : 18.3, lead_pct: 3, zinc_pct: 0.5, other_impurities_pct: 0.5 },
      wall_thickness: thick, tap_spectrum: spectrum,
      temperature_c: 23.5, ambient_humidity_pct: 54, sensor_ids: ['XRF-001', 'UT-002', 'MIC-003', 'ENV-004'],
    };
  }

  buildSpectrumFromModes(modes) {
    const freqs = modes.map(m => m.frequency_hz);
    const out = [];
    for (let f = 50; f <= 3000; f += (3000 - 50) / 256) {
      let amp = -65 - Math.random() * 5;
      freqs.forEach((base, i) => {
        const d = Math.abs(f - base), w = 4 + 2 * i;
        if (d < 40) amp = Math.max(amp, 14 - 2.8 * i - (d / w) ** 2 * 5);
      });
      out.push({ frequency_hz: +f.toFixed(2), amplitude_db: +Math.max(-80, amp).toFixed(2) });
    }
    return out;
  }

  connectWS() {
    const u = API.replace('http', 'ws').replace('/api', '') + '/api/alarms/stream';
    try {
      this.ws = new WebSocket(u);
      this.ws.onmessage = e => { try { this.pushAlarm(JSON.parse(e.data)); } catch {} };
      this.ws.onopen = () => document.getElementById('connect-ws').textContent = '✅ 告警流已连接';
      this.ws.onclose = () => document.getElementById('connect-ws').textContent = '🔌 已断开，重连';
    } catch (e) { alert('WebSocket连接失败: ' + e.message); }
  }

  bindUI() {
    document.getElementById('drum-select').addEventListener('change', e => this.selectDrum(e.target.value));
    const sliders = [
      ['pour-temp', 'pour-temp-val', '℃'],
      ['mold-temp', 'mold-temp-val', '℃'],
      ['cooling-time', 'cooling-time-val', '分'],
      ['tin-content', 'tin-val', '%'],
    ];
    sliders.forEach(([sid, lid, sfx]) => {
      const s = document.getElementById(sid), l = document.getElementById(lid);
      if (s && l) s.addEventListener('input', () => l.textContent = s.value + sfx);
    });

    document.querySelectorAll('.tab-btn').forEach(b => b.addEventListener('click', () => {
      document.querySelectorAll('.tab-btn').forEach(btn => btn.classList.toggle('active', btn === b));
      this.switchView(b.dataset.tab);
    }));

    document.getElementById('mode-select').addEventListener('change', e => {
      this.drum3D.setModeIndex(+e.target.value || 0);
    });

    document.getElementById('anim-toggle').addEventListener('change', e => {
      this.drum3D.setAnimating(e.target.checked);
    });

    document.getElementById('run-casting').addEventListener('click', async () => {
      const btn = document.getElementById('run-casting');
      btn.disabled = true; btn.textContent = '⏳ 仿真中...';
      const sn = +document.getElementById('tin-content').value;
      const res = await this.apiPost('/casting/simulate', {
        drum_id: this.state.currentDrumId,
        alloy: { copper_pct: 100 - sn - 3 - 0.5 - 0.5, tin_pct: sn, lead_pct: 3, zinc_pct: 0.5, other_impurities_pct: 0.5 },
        pour_temperature_c: +document.getElementById('pour-temp').value,
        mold_temperature_c: +document.getElementById('mold-temp').value,
        cooling_time_s: +document.getElementById('cooling-time').value * 60,
        mesh_resolution: 48,
      });
      if (res.success && res.data) {
        this.applyCastingResult(res.data); this.switchView('defects');
        res.data.defects.forEach(d => {
          if (d.severity >= 0.5) this.pushAlarm({
            alarm_type: 'ShrinkageDefect',
            severity: d.severity > 0.8 ? 'Critical' : 'Warning',
            message: `[${d.zone}] ${d.description}`, measured_value: d.severity, threshold_value: 0.5,
            metadata: d, timestamp: new Date().toISOString(), alarm_id: crypto.randomUUID ? crypto.randomUUID() : Math.random().toString(36),
          });
        });
        if (['CRITICAL', 'HIGH'].includes(res.data.overall_risk))
          this.pushAlarm({ alarm_type: 'StructuralFailureRisk',
            severity: res.data.overall_risk === 'CRITICAL' ? 'Fatal' : 'Critical',
            message: `整体铸造风险${res.data.overall_risk}，品质${(res.data.quality_score * 100).toFixed(0)}分`,
            measured_value: res.data.quality_score, threshold_value: 0.6, metadata: res.data,
            timestamp: new Date().toISOString(), alarm_id: Math.random().toString(36)
          });
      } else alert('仿真失败: ' + (res.error || '网络错误'));
      btn.disabled = false; btn.textContent = '🏺 运行铸造仿真';
    });

    document.getElementById('run-acoustic').addEventListener('click', async () => {
      const btn = document.getElementById('run-acoustic');
      btn.disabled = true; btn.textContent = '⏳ FEM计算中...';
      const res = await this.apiPost('/acoustics/analyze', { drum_id: this.state.currentDrumId, use_sensor_calibration: true });
      if (res.success && res.data) {
        this.applyAcousticResult(res.data); this.switchView('modes');
        if (!this.state.spectrum.length) this.state.spectrum = this.buildSpectrumFromModes(res.data.vibration_modes);
        this.refreshAllCharts();
        if (res.data.sound_quality_metric < 0.5)
          this.pushAlarm({ alarm_type: 'SoundQualityDegradation',
            severity: res.data.sound_quality_metric < 0.3 ? 'Critical' : 'Warning',
            message: `声学品质偏低: ${(res.data.sound_quality_metric * 100).toFixed(0)}/100，音准匹配度不足`,
            measured_value: res.data.sound_quality_metric, threshold_value: 0.5, metadata: res.data,
            timestamp: new Date().toISOString(), alarm_id: Math.random().toString(36)
          });
      } else alert('分析失败: ' + (res.error || '网络错误'));
      btn.disabled = false; btn.textContent = '🔊 进行声学分析';
    });

    document.getElementById('mock-reading').addEventListener('click', async () => {
      const reading = this.mockReading(this.state.currentDrumId, false);
      const res = await this.apiPost('/sensor/readings', reading);
      if (res.success) { this.applySensorReading(reading); (res.data || []).forEach(a => this.pushAlarm(a)); }
    });

    document.getElementById('inject-fault').addEventListener('click', async () => {
      const reading = this.mockReading(this.state.currentDrumId, true);
      const res = await this.apiPost('/sensor/readings', reading);
      if (res.success) { this.applySensorReading(reading); (res.data || []).forEach(a => this.pushAlarm(a)); }
    });

    document.getElementById('connect-ws').addEventListener('click', () => this.connectWS());
    window.addEventListener('resize', () => this.onResize());
  }

  onResize() {
    this.drum3D.onResize();
    this.refreshAllCharts();
  }

  init() {
    this.bindUI();
    this.onResize();
    this.switchView('3d');
    this.drum3D.buildBronzeDrum(78.5, 52.3);
    this.loadDrums();
    setInterval(() => {
      this.drawAlloyChart();
      this.drawThicknessHeatmap();
    }, 2000);
  }
}
