// ============================================================
// 古代铜鼓铸造工艺仿真与声学特性分析系统 - 前端主程序
// ============================================================

import * as THREE from 'three';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';

const API = (window.location.protocol === 'file:')
  ? 'http://127.0.0.1:8080/api'
  : '/api';

const state = {
  drums: [],
  currentDrum: null,
  currentDrumId: null,
  currentView: '3d',
  castingResult: null,
  acousticResult: null,
  modes: [],
  currentModeIndex: 0,
  shrinkageMap: [],
  coolingRateMap: [],
  defects: [],
  soundField: [],
  wallThickness: [],
  alarms: [],
  spectrum: [],
  animating: true,
  time: 0,
  currentDrumAlloy: null,
};

// ============================================================
// Three.js 初始化
// ============================================================
const canvas = document.getElementById('three-canvas');
const renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true });
renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
renderer.setSize(canvas.clientWidth, canvas.clientHeight, false);
renderer.outputColorSpace = THREE.SRGBColorSpace;
renderer.toneMapping = THREE.ACESFilmicToneMapping;
renderer.toneMappingExposure = 1.1;

const scene = new THREE.Scene();
scene.fog = new THREE.FogExp2(0x0f172a, 0.03);

const camera = new THREE.PerspectiveCamera(45, 1, 0.1, 1000);
camera.position.set(4, 3, 6);

const controls = new OrbitControls(camera, canvas);
controls.enableDamping = true;
controls.dampingFactor = 0.08;
controls.minDistance = 2;
controls.maxDistance = 20;

const ambient = new THREE.AmbientLight(0x404060, 0.4);
scene.add(ambient);
const keyLight = new THREE.DirectionalLight(0xffeebb, 1.2);
keyLight.position.set(5, 8, 5);
scene.add(keyLight);
const rimLight = new THREE.DirectionalLight(0x8888ff, 0.5);
rimLight.position.set(-6, 3, -4);
scene.add(rimLight);
const fillLight = new THREE.PointLight(0xffaa44, 0.8, 20);
fillLight.position.set(-3, 2, 4);
scene.add(fillLight);

const ground = new THREE.Mesh(
  new THREE.CircleGeometry(10, 64),
  new THREE.MeshStandardMaterial({ color: 0x1e293b, roughness: 0.95, metalness: 0.1 })
);
ground.rotation.x = -Math.PI / 2;
ground.position.y = -1.5;
scene.add(ground);

const pedestal = new THREE.Mesh(
  new THREE.CylinderGeometry(1.3, 1.6, 0.3, 64),
  new THREE.MeshStandardMaterial({ color: 0x3f3f46, roughness: 0.7, metalness: 0.4 })
);
pedestal.position.y = -1.3;
scene.add(pedestal);

const drumGroup = new THREE.Group();
scene.add(drumGroup);

let drumMesh = null, rimMesh = null, sideMesh = null;
let frogMeshes = [];
let soundFieldGroup = null, defectMarkers = [];

// ============================================================
// 构建铜鼓模型
// ============================================================
function buildBronzeDrum(diameterCm = 78.5, heightCm = 52.3) {
  while (drumGroup.children.length) {
    const c = drumGroup.children[0];
    if (c.geometry) c.geometry.dispose();
    if (c.material) c.material.dispose();
    drumGroup.remove(c);
  }
  frogMeshes = [];
  defectMarkers = [];
  if (soundFieldGroup) {
    while (soundFieldGroup.children.length) soundFieldGroup.remove(soundFieldGroup.children[0]);
    scene.remove(soundFieldGroup);
    soundFieldGroup = null;
  }

  const scale = 0.025;
  const radius = diameterCm * scale / 2;
  const height = heightCm * scale;
  const thickness = radius * 0.05;

  const bronzeMaterial = new THREE.MeshStandardMaterial({
    color: 0xb8860b, metalness: 0.92, roughness: 0.32, envMapIntensity: 1.2
  });

  const faceGeom = new THREE.CylinderGeometry(radius, radius, thickness * 0.6, 96, 16, false);
  const pos = faceGeom.attributes.position;
  for (let i = 0; i < pos.count; i++) {
    const y = pos.getY(i);
    if (y > 0) {
      const x = pos.getX(i), z = pos.getZ(i);
      const r = Math.sqrt(x * x + z * z);
      const dome = Math.exp(-Math.pow(r / (radius * 0.7), 2)) * thickness * 0.35;
      pos.setY(i, y + dome);
    }
  }
  faceGeom.computeVertexNormals();
  drumMesh = new THREE.Mesh(faceGeom, bronzeMaterial.clone());
  drumGroup.add(drumMesh);

  const rimGeom = new THREE.TorusGeometry(radius - thickness * 0.3, thickness * 0.7, 16, 96);
  rimMesh = new THREE.Mesh(rimGeom, new THREE.MeshStandardMaterial({
    color: 0x9a6a08, metalness: 0.9, roughness: 0.25
  }));
  rimMesh.position.y = thickness * 0.2;
  drumGroup.add(rimMesh);

  const sidePts = [];
  const segments = 96;
  for (let i = 0; i <= segments; i++) {
    const t = i / segments;
    const waistT = Math.sin(t * Math.PI);
    const localR = radius * (1.02 - 0.12 * waistT) * (1 - 0.08 * t * t);
    sidePts.push(new THREE.Vector2(localR, -height + t * height));
  }
  const sideGeom = new THREE.LatheGeometry(sidePts, 96);
  sideMesh = new THREE.Mesh(sideGeom, bronzeMaterial.clone());
  sideMesh.position.y = thickness * 0.1;
  drumGroup.add(sideMesh);

  // 太阳纹
  const sunRays = new THREE.Group();
  for (let r = 0; r < 12; r++) {
    const angle = (r / 12) * Math.PI * 2;
    const ray = new THREE.Mesh(
      new THREE.BoxGeometry(radius * 0.03, thickness * 0.8, radius * 0.3),
      new THREE.MeshStandardMaterial({ color: 0xd4a015, metalness: 0.95, roughness: 0.2 })
    );
    ray.position.set(Math.cos(angle) * radius * 0.18, thickness * 0.6, Math.sin(angle) * radius * 0.18);
    ray.rotation.y = angle;
    sunRays.add(ray);
  }
  const boss = new THREE.Mesh(
    new THREE.SphereGeometry(radius * 0.08, 32, 32),
    new THREE.MeshStandardMaterial({ color: 0xf0c040, metalness: 0.98, roughness: 0.15 })
  );
  boss.position.y = thickness * 0.85;
  sunRays.add(boss);
  drumGroup.add(sunRays);

  // 晕圈
  for (let ring = 0; ring < 3; ring++) {
    const rr = radius * (0.35 + ring * 0.2);
    const torus = new THREE.Mesh(
      new THREE.TorusGeometry(rr, thickness * 0.04, 8, 96),
      new THREE.MeshStandardMaterial({ color: 0xa87506, metalness: 0.9, roughness: 0.3 })
    );
    torus.position.y = thickness * 0.45;
    torus.rotation.x = Math.PI / 2;
    drumGroup.add(torus);
  }

  // 立蛙
  for (let f = 0; f < 4; f++) {
    const angle = (f / 4) * Math.PI * 2 + Math.PI / 4;
    const frog = createFrog(thickness);
    frog.position.set(Math.cos(angle) * radius * 0.85, thickness * 0.4, Math.sin(angle) * radius * 0.85);
    frog.rotation.y = -angle;
    drumGroup.add(frog);
    frogMeshes.push(frog);
  }

  // 双耳
  for (let e = 0; e < 2; e++) {
    const angle = e * Math.PI;
    const earGroup = new THREE.Group();
    earGroup.add(new THREE.Mesh(
      new THREE.TorusGeometry(thickness * 2.5, thickness * 0.25, 16, 48),
      new THREE.MeshStandardMaterial({ color: 0x996600, metalness: 0.9, roughness: 0.3 })
    ));
    earGroup.add(new THREE.Mesh(
      new THREE.TorusGeometry(thickness * 1.6, thickness * 0.18, 16, 48),
      new THREE.MeshStandardMaterial({ color: 0x996600, metalness: 0.9, roughness: 0.3 })
    ));
    earGroup.position.set(Math.cos(angle) * (radius + thickness * 1.5), -height * 0.35, Math.sin(angle) * (radius + thickness * 1.5));
    earGroup.rotation.z = Math.PI / 2;
    earGroup.rotation.y = angle + Math.PI / 2;
    drumGroup.add(earGroup);
  }

  // 圈足
  const foot = new THREE.Mesh(
    new THREE.CylinderGeometry(radius * 1.02, radius * 1.08, thickness * 1.2, 96),
    new THREE.MeshStandardMaterial({ color: 0x7a5600, metalness: 0.88, roughness: 0.35 })
  );
  foot.position.y = -height - thickness * 0.5;
  drumGroup.add(foot);

  controls.target.set(0, -height * 0.2, 0);
  return { radius, height, thickness };
}

function createFrog(thickness) {
  const g = new THREE.Group();
  const mat = new THREE.MeshStandardMaterial({ color: 0x9a6a08, metalness: 0.9, roughness: 0.3 });
  const body = new THREE.Mesh(new THREE.SphereGeometry(thickness * 0.9, 16, 12), mat);
  body.scale.set(1, 0.7, 1.4); body.position.y = thickness * 0.7;
  g.add(body);
  const head = new THREE.Mesh(new THREE.SphereGeometry(thickness * 0.5, 16, 12), mat);
  head.position.set(0, thickness * 0.9, thickness * 1.0);
  head.scale.set(1, 0.8, 1); g.add(head);
  for (const [x, z, s] of [[-0.4, -0.6, 0.3], [0.4, -0.6, 0.3], [-0.4, 0.6, 0.35], [0.4, 0.6, 0.35]]) {
    const leg = new THREE.Mesh(new THREE.SphereGeometry(thickness * s, 10, 8), mat);
    leg.position.set(x * thickness, thickness * 0.3, z * thickness);
    g.add(leg);
  }
  const eyeMat = new THREE.MeshStandardMaterial({ color: 0x111 });
  for (const x of [-0.15, 0.15]) {
    const eye = new THREE.Mesh(new THREE.SphereGeometry(thickness * 0.08, 8, 8), eyeMat);
    eye.position.set(x * thickness, thickness * 1.05, thickness * 1.35);
    g.add(eye);
  }
  return g;
}

// ============================================================
// 视图切换
// ============================================================
function switchView(viewName) {
  state.currentView = viewName;
  document.querySelectorAll('.tab-btn').forEach(b => b.classList.toggle('active', b.dataset.tab === viewName));
  if (soundFieldGroup) soundFieldGroup.visible = viewName === 'soundfield';
  defectMarkers.forEach(m => m.visible = viewName === 'defects');

  const ov = document.getElementById('overlay-text');
  const legend = document.getElementById('legend-bars');
  legend.innerHTML = '';

  switch (viewName) {
    case '3d':
      ov.textContent = '铜鼓三维几何模型（可拖拽旋转/滚轮缩放）';
      if (drumMesh) drumMesh.visible = true;
      break;
    case 'modes':
      ov.textContent = `模态振型动画：${state.modes[state.currentModeIndex]?.node_pattern || '请先运行声学分析'}`;
      if (drumMesh) drumMesh.visible = true;
      buildLegend(legend, '位移幅度', ['#3b82f6', '#8b5cf6', '#ec4899', '#ef4444'], ['0', '0.33', '0.66', '1']);
      break;
    case 'soundfield':
      ov.textContent = '远场声辐射声压云图（SPL dB）';
      if (drumMesh) drumMesh.visible = true;
      if (!soundFieldGroup || !soundFieldGroup.children.length) buildSoundField();
      if (soundFieldGroup) soundFieldGroup.visible = true;
      buildLegend(legend, 'SPL (dB)', ['#1e3a8a', '#3b82f6', '#10b981', '#eab308', '#ef4444'], ['40', '60', '80', '100', '120']);
      break;
    case 'defects':
      ov.textContent = `铸造缺陷分布预测：共 ${state.defects.length} 处`;
      if (drumMesh) drumMesh.visible = true;
      if (state.defects.length && !defectMarkers.length) buildDefectMarkers();
      defectMarkers.forEach(m => m.visible = true);
      buildLegend(legend, '缺陷严重度', ['#22c55e', '#eab308', '#f97316', '#ef4444', '#991b1b'], ['低', '中', '中高', '高', '致命']);
      break;
  }
}

function buildLegend(container, title, colors, labels) {
  container.innerHTML = `<div class="text-slate-300 font-semibold mb-1">${title}</div>`;
  for (let i = colors.length - 1; i >= 0; i--) {
    container.innerHTML += `<div class="flex items-center gap-2"><div class="w-4 h-3 rounded" style="background:${colors[i]}"></div><span class="text-slate-400">${labels[i]}</span></div>`;
  }
}

// ============================================================
// 振动模态动画
// ============================================================
function updateModeAnimation(timeSec) {
  if (state.currentView !== 'modes' || !state.modes.length || !drumMesh) return;
  const mode = state.modes[state.currentModeIndex];
  if (!mode) return;

  const freq = mode.frequency_hz;
  const phase = timeSec * freq * Math.PI * 2;
  const amp = 0.03 * (1 / (1 + mode.mode_order * 0.3));
  const displacements = mode.modal_displacements || [];

  const pos = drumMesh.geometry.attributes.position;
  const count = pos.count;
  if (!drumMesh.userData.originalY) {
    drumMesh.userData.originalY = new Float32Array(count);
    for (let i = 0; i < count; i++) drumMesh.userData.originalY[i] = pos.getY(i);
  }
  const res = Math.ceil(Math.sqrt(displacements.length));

  const colors = new Float32Array(count * 3);
  for (let i = 0; i < count; i++) {
    const x = pos.getX(i), z = pos.getZ(i);
    const R = drumMesh.geometry.parameters?.radiusTop || 1;
    const xi = Math.floor(((x / R + 1) / 2) * res);
    const zi = Math.floor(((z / R + 1) / 2) * res);
    const idx = Math.min(displacements.length - 1, Math.max(0, zi * res + xi));
    const w = displacements[idx] ? displacements[idx][2] : 0;
    const origY = drumMesh.userData.originalY[i];
    const use = origY > -0.001 ? 1 : 0;
    const dy = Math.sin(phase) * amp * (Math.abs(w) * 3 + 0.3) * use;
    pos.setY(i, origY + dy);

    const norm = Math.min(1, Math.abs(dy) / amp * 0.5);
    const col = new THREE.Color().setHSL(0.65 - norm * 0.65, 0.85, 0.5 + norm * 0.2);
    colors[i*3] = col.r; colors[i*3+1] = col.g; colors[i*3+2] = col.b;
  }
  pos.needsUpdate = true;
  drumMesh.geometry.computeVertexNormals();
  drumMesh.geometry.setAttribute('color', new THREE.BufferAttribute(colors, 3));
  drumMesh.material.vertexColors = true;

  frogMeshes.forEach((f, idx) => {
    f.position.y = (f.userData.baseY = f.userData.baseY ?? f.position.y) + Math.sin(phase + idx) * amp * 0.5;
  });
}

function resetModeDeformation() {
  if (!drumMesh || !drumMesh.userData.originalY) return;
  const pos = drumMesh.geometry.attributes.position;
  for (let i = 0; i < pos.count; i++) pos.setY(i, drumMesh.userData.originalY[i]);
  pos.needsUpdate = true;
  drumMesh.geometry.computeVertexNormals();
  drumMesh.material.vertexColors = false;
  drumMesh.geometry.deleteAttribute('color');
  frogMeshes.forEach(f => { if (f.userData.baseY != null) f.position.y = f.userData.baseY; });
}

// ============================================================
// 声场云图
// ============================================================
function buildSoundField() {
  if (!state.soundField.length) return;
  if (soundFieldGroup) scene.remove(soundFieldGroup);
  soundFieldGroup = new THREE.Group();
  const points = state.soundField;
  let maxSpl = 0, minSpl = Infinity;
  points.forEach(p => { maxSpl = Math.max(maxSpl, p.spl_db); minSpl = Math.min(minSpl, p.spl_db); });
  const range = Math.max(1, maxSpl - minSpl);
  const glowTex = makeGlowTexture();

  points.forEach(p => {
    const norm = (p.spl_db - minSpl) / range;
    const col = new THREE.Color().setHSL(0.65 - norm * 0.65, 0.9, 0.55);
    const size = 0.06 + norm * 0.14;
    const mat = new THREE.SpriteMaterial({ map: glowTex, transparent: true, depthWrite: false, blending: THREE.AdditiveBlending, color: col, opacity: 0.75 });
    const sprite = new THREE.Sprite(mat);
    sprite.position.set((p.x - 0.5) * 10, (p.z - 0.5) * 10 + 1.5, (p.y - 0.5) * 10);
    sprite.scale.setScalar(size * 3);
    soundFieldGroup.add(sprite);
  });

  const shell = new THREE.Mesh(
    new THREE.SphereGeometry(3.5, 32, 16, 0, Math.PI * 2, 0, Math.PI / 2),
    new THREE.MeshBasicMaterial({ color: 0x3b82f6, transparent: true, opacity: 0.04, wireframe: true })
  );
  soundFieldGroup.add(shell);
  scene.add(soundFieldGroup);
  soundFieldGroup.visible = state.currentView === 'soundfield';
}

function makeGlowTexture() {
  const c = document.createElement('canvas');
  c.width = c.height = 64;
  const g = c.getContext('2d');
  const grd = g.createRadialGradient(32, 32, 0, 32, 32, 32);
  grd.addColorStop(0, 'rgba(255,255,255,1)');
  grd.addColorStop(0.3, 'rgba(255,255,255,0.6)');
  grd.addColorStop(1, 'rgba(255,255,255,0)');
  g.fillStyle = grd; g.fillRect(0, 0, 64, 64);
  return new THREE.CanvasTexture(c);
}

// ============================================================
// 缺陷标记
// ============================================================
function buildDefectMarkers() {
  if (!drumMesh || !state.defects.length) return;
  const R = drumMesh.geometry.parameters?.radiusTop || 1;
  state.defects.forEach(def => {
    const col = sevColor(def.severity);
    const g = new THREE.Group();
    const ring = new THREE.Mesh(
      new THREE.RingGeometry(0.04, 0.08, 32),
      new THREE.MeshBasicMaterial({ color: col, transparent: true, opacity: 0.9, side: THREE.DoubleSide })
    );
    ring.rotation.x = -Math.PI / 2;
    g.add(ring);
    const pulse = new THREE.Mesh(
      new THREE.RingGeometry(0.08, 0.1, 32),
      new THREE.MeshBasicMaterial({ color: col, transparent: true, opacity: 0.5, side: THREE.DoubleSide })
    );
    pulse.rotation.x = -Math.PI / 2;
    pulse.userData.isPulse = true;
    g.add(pulse);
    const pillar = new THREE.Mesh(
      new THREE.CylinderGeometry(0.005, 0.005, 0.3, 8),
      new THREE.MeshBasicMaterial({ color: col, transparent: true, opacity: 0.7 })
    );
    pillar.position.y = 0.15;
    g.add(pillar);
    g.position.set((def.x_frac - 0.5) * 2 * R * 0.95, 0.03, (def.y_frac - 0.5) * 2 * R * 0.95);
    g.userData = { defect };
    drumGroup.add(g);
    defectMarkers.push(g);
    g.visible = state.currentView === 'defects';
  });
}

function sevColor(sev) {
  if (sev >= 0.85) return 0x991b1b;
  if (sev >= 0.7) return 0xef4444;
  if (sev >= 0.5) return 0xf97316;
  if (sev >= 0.3) return 0xeab308;
  return 0x22c55e;
}

function updateDefectPulse(t) {
  if (state.currentView !== 'defects') return;
  defectMarkers.forEach(g => {
    g.children.forEach(c => {
      if (c.userData.isPulse) {
        const s = 1 + 0.6 * Math.sin(t * 4 + g.position.x);
        c.scale.setScalar(s);
        c.material.opacity = 0.5 * (1 - 0.5 * Math.abs(Math.sin(t * 4)));
      }
    });
  });
}

// ============================================================
// 动画循环
// ============================================================
const startTime = performance.now();
function animate() {
  requestAnimationFrame(animate);
  state.time = (performance.now() - startTime) / 1000;
  if (state.animating) {
    if (state.currentView === 'modes') updateModeAnimation(state.time);
    else if (state.currentView !== 'modes') resetModeDeformation();
    if (state.currentView === 'defects') updateDefectPulse(state.time);
  } else {
    resetModeDeformation();
  }
  controls.update();
  renderer.render(scene, camera);
}

// ============================================================
// Canvas 图表
// ============================================================
function drawGauge(id, value, max = 100, label = '') {
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

function shade(hex, pct) {
  const f = parseInt(hex.slice(1), 16);
  const t = pct < 0 ? 0 : 255, p = Math.abs(pct);
  const R = f >> 16, G = (f >> 8) & 0xff, B = f & 0xff;
  return '#' + (0x1000000 + (Math.round((t - R) * p) + R) * 0x10000 + (Math.round((t - G) * p) + G) * 0x100 + (Math.round((t - B) * p) + B)).toString(16).slice(1);
}

function drawAlloyChart() {
  const c = document.getElementById('alloy-chart'); if (!c) return;
  const dpr = window.devicePixelRatio || 1;
  c.width = c.clientWidth * dpr; c.height = c.clientHeight * dpr;
  const ctx = c.getContext('2d'); ctx.scale(dpr, dpr);
  const w = c.clientWidth, h = c.clientHeight;
  ctx.clearRect(0, 0, w, h);
  const a = state.currentDrumAlloy || { copper_pct: 78, tin_pct: 18, lead_pct: 3, zinc_pct: 0.5, other_impurities_pct: 0.5 };
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
    grd.addColorStop(0, it.c); grd.addColorStop(1, shade(it.c, -0.3));
    ctx.fillStyle = grd; ctx.fillRect(x, 0, bw, h * 0.6);
    ctx.fillStyle = '#f1f5f9'; ctx.font = 'bold 11px sans-serif'; ctx.textAlign = 'center';
    ctx.fillText(`${it.v.toFixed(1)}%`, x + bw / 2, h * 0.35);
    ctx.font = '10px sans-serif'; ctx.fillStyle = '#94a3b8';
    ctx.fillText(it.k, x + bw / 2, h * 0.6 + 14);
    x += bw;
  });
}

function drawSpectrumChart() {
  const c = document.getElementById('spectrum-chart'); if (!c) return;
  const dpr = window.devicePixelRatio || 1;
  c.width = c.clientWidth * dpr; c.height = c.clientHeight * dpr;
  const ctx = c.getContext('2d'); ctx.scale(dpr, dpr);
  const w = c.clientWidth, h = c.clientHeight;
  ctx.clearRect(0, 0, w, h);
  const d = state.spectrum;
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

function drawFrequencyChart() {
  const c = document.getElementById('frequency-chart'); if (!c) return;
  const dpr = window.devicePixelRatio || 1;
  c.width = c.clientWidth * dpr; c.height = c.clientHeight * dpr;
  const ctx = c.getContext('2d'); ctx.scale(dpr, dpr);
  const w = c.clientWidth, h = c.clientHeight;
  ctx.clearRect(0, 0, w, h);
  ctx.fillStyle = '#475569'; ctx.font = 'bold 10px sans-serif'; ctx.textAlign = 'center';
  ctx.fillText('固有频率 vs 参考音高 (Hz)', w / 2, 14);
  const freqs = state.acousticResult?.resonance_frequencies_hz || state.modes.map(m => m.frequency_hz) || [];
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

function drawThicknessHeatmap() {
  const c = document.getElementById('thickness-heatmap'); if (!c) return;
  const dpr = window.devicePixelRatio || 1;
  c.width = c.clientWidth * dpr; c.height = c.clientHeight * dpr;
  const ctx = c.getContext('2d'); ctx.scale(dpr, dpr);
  const w = c.clientWidth, h = c.clientHeight;
  ctx.clearRect(0, 0, w, h);
  const data = state.shrinkageMap.length ? state.shrinkageMap :
    state.wallThickness.map(t => [t.x_frac, t.y_frac, t.thickness_mm]);
  ctx.fillStyle = '#475569'; ctx.font = '11px sans-serif'; ctx.textAlign = 'center';
  if (!data.length) { ctx.fillText('暂无数据', w / 2, h / 2); return; }
  const res = Math.ceil(Math.sqrt(data.length));
  const cellW = w / res, cellH = h / res;
  const vs = data.map(d => d[2]);
  const mn = Math.min(...vs), mx = Math.max(...vs);
  const rng = mx - mn || 1;
  data.forEach(([x, y, v]) => {
    ctx.fillStyle = colormap((v - mn) / rng, state.shrinkageMap.length ? 'hot' : 'viridis');
    ctx.fillRect(x * w - cellW / 2, y * h - cellH / 2, cellW + 1, cellH + 1);
  });
  ctx.strokeStyle = 'rgba(234,179,8,0.6)'; ctx.lineWidth = 1.5;
  ctx.beginPath();
  ctx.ellipse(w / 2, h / 2, w * 0.46, h * 0.46, 0, 0, Math.PI * 2);
  ctx.stroke();
}

function colormap(t, kind = 'viridis') {
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

function refreshAllCharts() {
  drawAlloyChart();
  drawSpectrumChart();
  drawFrequencyChart();
  drawThicknessHeatmap();
  const qc = state.castingResult?.quality_score ?? 0;
  const aq = state.acousticResult?.sound_quality_metric ?? 0;
  const score = state.castingResult || state.acousticResult ? ((qc || 0.6) + (aq || 0.6)) / 2 * 100 : 70;
  drawGauge('quality-gauge', score, 100, '综合品质分');
  document.getElementById('cast-quality').textContent = state.castingResult ? (qc * 100).toFixed(0) : '--';
  document.getElementById('acoustic-quality').textContent = state.acousticResult ? (aq * 100).toFixed(0) : '--';
  document.getElementById('defect-count').textContent = state.defects.length || '0';
  const rl = document.getElementById('risk-level');
  rl.textContent = state.castingResult?.overall_risk || '--';
  rl.className = 'text-xl font-bold ' + ({
    CRITICAL: 'text-red-400', HIGH: 'text-orange-400', MEDIUM: 'text-yellow-400', LOW: 'text-green-400'
  }[state.castingResult?.overall_risk] || 'text-slate-200');
}

// ============================================================
// API
// ============================================================
async function apiGet(path) {
  try { const r = await fetch(`${API}${path}`); return await r.json(); }
  catch (e) { return { success: false, error: String(e) }; }
}
async function apiPost(path, body) {
  try {
    const r = await fetch(`${API}${path}`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
    return await r.json();
  } catch (e) { return { success: false, error: String(e) }; }
}

async function loadDrums() {
  const res = await apiGet('/drums');
  if (res.success && res.data) {
    state.drums = res.data;
    const sel = document.getElementById('drum-select');
    sel.innerHTML = res.data.map(d => `<option value="${d.drum_id}">${d.name} [${d.ethnic_group}]</option>`).join('');
    if (res.data.length) {
      sel.value = res.data[0].drum_id;
      selectDrum(res.data[0].drum_id);
    }
  }
  document.getElementById('last-update').textContent = '已连接 ' + new Date().toLocaleTimeString();
}

function selectDrum(id) {
  const drum = state.drums.find(d => d.drum_id === id);
  if (!drum) return;
  state.currentDrum = drum;
  state.currentDrumId = id;
  document.getElementById('dim-diameter').textContent = drum.diameter_cm.toFixed(1) + 'cm';
  document.getElementById('dim-height').textContent = drum.height_cm.toFixed(1) + 'cm';
  document.getElementById('dim-mass').textContent = drum.mass_kg.toFixed(1) + 'kg';
  buildBronzeDrum(drum.diameter_cm, drum.height_cm);
  state.shrinkageMap = []; state.defects = []; state.castingResult = null;
  state.acousticResult = null; state.modes = []; state.soundField = [];
  state.spectrum = []; state.wallThickness = [];
  refreshAllCharts(); populateModeSelect();
  apiGet(`/sensor/readings/${id}`).then(r => { if (r.success && r.data?.[0]) applySensorReading(r.data[0]); });
  apiGet(`/casting/${id}`).then(r => { if (r.success && r.data) applyCastingResult(r.data); });
  apiGet(`/acoustics/${id}`).then(r => { if (r.success && r.data) applyAcousticResult(r.data); });
  apiGet(`/alarms/${id}`).then(r => { if (r.success && r.data) { state.alarms = r.data; renderAlarms(); } });
}

function populateModeSelect() {
  const sel = document.getElementById('mode-select');
  sel.innerHTML = state.modes.length
    ? state.modes.map(m => `<option value="${m.mode_order - 1}">M${m.mode_order}: ${m.frequency_hz.toFixed(1)}Hz ${m.node_pattern}</option>`).join('')
    : '<option>暂无模态数据</option>';
}

function applySensorReading(r) {
  state.currentDrumAlloy = r.alloy;
  state.spectrum = r.tap_spectrum;
  state.wallThickness = r.wall_thickness;
  refreshAllCharts();
}
function applyCastingResult(r) {
  state.castingResult = r;
  state.shrinkageMap = r.shrinkage_risk_map || [];
  state.defects = r.defects || [];
  if (state.defects.length) buildDefectMarkers();
  state.currentDrumAlloy = r.alloy;
  refreshAllCharts();
}
function applyAcousticResult(r) {
  state.acousticResult = r;
  state.modes = r.vibration_modes || [];
  state.soundField = r.sound_field || [];
  if (state.soundField.length) buildSoundField();
  populateModeSelect();
  refreshAllCharts();
}

function renderAlarms() {
  const list = document.getElementById('alarm-list');
  document.getElementById('alarm-count').textContent = state.alarms.length;
  if (!state.alarms.length) {
    list.innerHTML = `<div class="text-slate-500 text-center py-8">暂无告警</div>`;
    return;
  }
  const cols = { Info: 'bg-blue-500/20 border-blue-500/40 text-blue-200', Warning: 'bg-yellow-500/20 border-yellow-500/40 text-yellow-200', Critical: 'bg-orange-500/20 border-orange-500/40 text-orange-200', Fatal: 'bg-red-600/30 border-red-500/60 text-red-200' };
  list.innerHTML = state.alarms.slice(0, 20).map(a => `
    <div class="border rounded-lg px-3 py-2 ${cols[a.severity] || cols.Info}">
      <div class="flex justify-between items-center gap-2 mb-1">
        <span class="font-semibold truncate">${a.alarm_type}</span>
        <span class="text-[10px] opacity-75">${new Date(a.timestamp).toLocaleString()}</span>
      </div>
      <div class="text-[11px] leading-snug opacity-90">${a.message}</div>
    </div>
  `).join('');
}

// ============================================================
// 事件绑定
// ============================================================
function bindUI() {
  document.getElementById('drum-select').addEventListener('change', e => selectDrum(e.target.value));
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
  document.querySelectorAll('.tab-btn').forEach(b => b.addEventListener('click', () => switchView(b.dataset.tab)));
  document.getElementById('mode-select').addEventListener('change', e => { state.currentModeIndex = +e.target.value || 0; });
  document.getElementById('anim-toggle').addEventListener('change', e => state.animating = e.target.checked);

  document.getElementById('run-casting').addEventListener('click', async () => {
    const btn = document.getElementById('run-casting');
    btn.disabled = true; btn.textContent = '⏳ 仿真中...';
    const sn = +document.getElementById('tin-content').value;
    const res = await apiPost('/casting/simulate', {
      drum_id: state.currentDrumId,
      alloy: { copper_pct: 100 - sn - 3 - 0.5 - 0.5, tin_pct: sn, lead_pct: 3, zinc_pct: 0.5, other_impurities_pct: 0.5 },
      pour_temperature_c: +document.getElementById('pour-temp').value,
      mold_temperature_c: +document.getElementById('mold-temp').value,
      cooling_time_s: +document.getElementById('cooling-time').value * 60,
      mesh_resolution: 48,
    });
    if (res.success && res.data) {
      applyCastingResult(res.data); switchView('defects');
      res.data.defects.forEach(d => {
        if (d.severity >= 0.5) pushAlarm({
          alarm_type: 'ShrinkageDefect',
          severity: d.severity > 0.8 ? 'Critical' : 'Warning',
          message: `[${d.zone}] ${d.description}`, measured_value: d.severity, threshold_value: 0.5,
          metadata: d, timestamp: new Date().toISOString(), alarm_id: crypto.randomUUID ? crypto.randomUUID() : Math.random().toString(36),
        });
      });
      if (['CRITICAL', 'HIGH'].includes(res.data.overall_risk))
        pushAlarm({ alarm_type: 'StructuralFailureRisk',
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
    const res = await apiPost('/acoustics/analyze', { drum_id: state.currentDrumId, use_sensor_calibration: true });
    if (res.success && res.data) {
      applyAcousticResult(res.data); switchView('modes');
      if (!state.spectrum.length) state.spectrum = buildSpectrumFromModes(res.data.vibration_modes);
      refreshAllCharts();
      if (res.data.sound_quality_metric < 0.5)
        pushAlarm({ alarm_type: 'SoundQualityDegradation',
          severity: res.data.sound_quality_metric < 0.3 ? 'Critical' : 'Warning',
          message: `声学品质偏低: ${(res.data.sound_quality_metric * 100).toFixed(0)}/100，音准匹配度不足`,
          measured_value: res.data.sound_quality_metric, threshold_value: 0.5, metadata: res.data,
          timestamp: new Date().toISOString(), alarm_id: Math.random().toString(36)
        });
    } else alert('分析失败: ' + (res.error || '网络错误'));
    btn.disabled = false; btn.textContent = '🔊 进行声学分析';
  });

  document.getElementById('mock-reading').addEventListener('click', async () => {
    const reading = mockReading(state.currentDrumId, false);
    const res = await apiPost('/sensor/readings', reading);
    if (res.success) { applySensorReading(reading); (res.data || []).forEach(pushAlarm); }
  });
  document.getElementById('inject-fault').addEventListener('click', async () => {
    const reading = mockReading(state.currentDrumId, true);
    const res = await apiPost('/sensor/readings', reading);
    if (res.success) { applySensorReading(reading); (res.data || []).forEach(pushAlarm); }
  });
  document.getElementById('connect-ws').addEventListener('click', connectWS);
  window.addEventListener('resize', onResize);
}

function pushAlarm(a) {
  state.alarms.unshift(a);
  if (state.alarms.length > 200) state.alarms.length = 200;
  renderAlarms();
}

function mockReading(drumId, fault = false) {
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

function buildSpectrumFromModes(modes) {
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

let ws = null;
function connectWS() {
  const u = API.replace('http', 'ws').replace('/api', '') + '/api/alarms/stream';
  try {
    ws = new WebSocket(u);
    ws.onmessage = e => { try { pushAlarm(JSON.parse(e.data)); } catch {} };
    ws.onopen = () => document.getElementById('connect-ws').textContent = '✅ 告警流已连接';
    ws.onclose = () => document.getElementById('connect-ws').textContent = '🔌 已断开，重连';
  } catch (e) { alert('WebSocket连接失败: ' + e.message); }
}

function onResize() {
  renderer.setSize(canvas.clientWidth, canvas.clientHeight, false);
  camera.aspect = canvas.clientWidth / canvas.clientHeight;
  camera.updateProjectionMatrix();
  refreshAllCharts();
}

// ============================================================
// 启动
// ============================================================
bindUI();
onResize();
switchView('3d');
buildBronzeDrum(78.5, 52.3);
animate();
loadDrums();

// 循环刷新图表
setInterval(() => {
  drawAlloyChart();
  drawThicknessHeatmap();
}, 2000);
