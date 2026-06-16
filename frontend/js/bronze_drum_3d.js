import * as THREE from 'three';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';

export class BronzeDrum3D {
  constructor(canvasId) {
    this.canvas = document.getElementById(canvasId);
    if (!this.canvas) throw new Error(`Canvas ${canvasId} not found`);

    this.state = {
      currentView: '3d',
      currentModeIndex: 0,
      modes: [],
      defects: [],
      soundField: [],
      shrinkageMap: [],
      wallThickness: [],
      animating: true,
      time: 0,
    };

    this.renderer = null;
    this.scene = null;
    this.camera = null;
    this.controls = null;
    this.drumGroup = null;
    this.drumMesh = null;
    this.rimMesh = null;
    this.sideMesh = null;
    this.frogMeshes = [];
    this.soundFieldGroup = null;
    this.defectMarkers = [];
    this.modeShaderMaterial = null;
    this.modeDisplacementTextures = [];
    this.modeAnimationUniforms = null;
    this.radius = 1.0;

    this.glowTexCache = null;
    this.startTime = 0;
    this.init();
  }

  init() {
    this.renderer = new THREE.WebGLRenderer({ canvas: this.canvas, antialias: true, alpha: true });
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    this.renderer.setSize(this.canvas.clientWidth, this.canvas.clientHeight, false);
    this.renderer.outputColorSpace = THREE.SRGBColorSpace;
    this.renderer.toneMapping = THREE.ACESFilmicToneMapping;
    this.renderer.toneMappingExposure = 1.1;

    this.scene = new THREE.Scene();
    this.scene.fog = new THREE.FogExp2(0x0f172a, 0.03);

    this.camera = new THREE.PerspectiveCamera(45, 1, 0.1, 1000);
    this.camera.position.set(4, 3, 6);

    this.controls = new OrbitControls(this.camera, this.canvas);
    this.controls.enableDamping = true;
    this.controls.dampingFactor = 0.08;
    this.controls.minDistance = 2;
    this.controls.maxDistance = 20;

    this.setupLights();
    this.setupEnvironment();

    this.drumGroup = new THREE.Group();
    this.scene.add(this.drumGroup);

    this.startTime = performance.now();
    this.animate();
  }

  setupLights() {
    const ambient = new THREE.AmbientLight(0x404060, 0.4);
    this.scene.add(ambient);
    const keyLight = new THREE.DirectionalLight(0xffeebb, 1.2);
    keyLight.position.set(5, 8, 5);
    this.scene.add(keyLight);
    const rimLight = new THREE.DirectionalLight(0x8888ff, 0.5);
    rimLight.position.set(-6, 3, -4);
    this.scene.add(rimLight);
    const fillLight = new THREE.PointLight(0xffaa44, 0.8, 20);
    fillLight.position.set(-3, 2, 4);
    this.scene.add(fillLight);
  }

  setupEnvironment() {
    const ground = new THREE.Mesh(
      new THREE.CircleGeometry(10, 64),
      new THREE.MeshStandardMaterial({ color: 0x1e293b, roughness: 0.95, metalness: 0.1 })
    );
    ground.rotation.x = -Math.PI / 2;
    ground.position.y = -1.5;
    this.scene.add(ground);

    const pedestal = new THREE.Mesh(
      new THREE.CylinderGeometry(1.3, 1.6, 0.3, 64),
      new THREE.MeshStandardMaterial({ color: 0x3f3f46, roughness: 0.7, metalness: 0.4 })
    );
    pedestal.position.y = -1.3;
    this.scene.add(pedestal);
  }

  modeVertexShader = /* glsl */`
    uniform float uTime;
    uniform float uModeIndex;
    uniform float uModeCount;
    uniform float uAmplitude;
    uniform float uFrequency;
    uniform sampler2D uDisplacementTex;
    uniform float uTexSize;
    uniform float uRadius;

    varying vec3 vColor;
    varying float vDisplacement;
    varying vec2 vPos;

    void main() {
      vec3 pos = position;
      vPos = position.xz;
      vec2 uv = (position.xz / uRadius + 1.0) * 0.5;
      vec4 dispSample = texture2D(uDisplacementTex, uv);
      float w = dispSample.r * 2.0 - 1.0;
      float phase = uTime * uFrequency * 6.28318;
      float dy = sin(phase) * uAmplitude * (abs(w) * 3.0 + 0.3);
      float use = step(-0.001, position.y);
      pos.y += dy * use;
      float norm = clamp(abs(dy) / uAmplitude * 0.5, 0.0, 1.0);
      float hue = 0.65 - norm * 0.65;
      vec3 c = hsl2rgb(hue, 0.85, 0.5 + norm * 0.2);
      vColor = c;
      vDisplacement = norm;
      gl_Position = projectionMatrix * modelViewMatrix * vec4(pos, 1.0);
    }
  `;

  modeFragmentShader = /* glsl */`
    uniform float uUseModeColors;
    varying vec3 vColor;
    varying float vDisplacement;
    varying vec2 vPos;

    void main() {
      if (uUseModeColors > 0.5) {
        gl_FragColor = vec4(vColor, 1.0);
      } else {
        vec3 bronze = vec3(0.72, 0.53, 0.04);
        gl_FragColor = vec4(bronze, 1.0);
      }
    }
  `;

  createModeShaderMaterial(baseMaterial) {
    const uniforms = {
      uTime: { value: 0 },
      uModeIndex: { value: 0 },
      uModeCount: { value: 1 },
      uAmplitude: { value: 0.03 },
      uFrequency: { value: 100 },
      uDisplacementTex: { value: null },
      uTexSize: { value: 24 },
      uRadius: { value: this.radius },
      uUseModeColors: { value: 0 },
    };

    const hslHelper = `
      vec3 hsl2rgb(float h, float s, float l) {
        vec3 rgb = clamp(abs(mod(h*6.0+vec3(0.0,4.0,2.0),6.0)-3.0)-1.0, 0.0, 1.0);
        return l + s * (rgb - 0.5) * (1.0 - abs(2.0*l - 1.0));
      }
    `;

    const mat = new THREE.ShaderMaterial({
      uniforms,
      vertexShader: hslHelper + this.modeVertexShader,
      fragmentShader: this.modeFragmentShader,
      vertexColors: false,
    });

    if (baseMaterial) {
      mat.metalness = baseMaterial.metalness || 0.9;
      mat.roughness = baseMaterial.roughness || 0.3;
    }

    return mat;
  }

  buildModeDisplacementTextures(modes, radius) {
    this.modeDisplacementTextures.forEach(t => t.dispose());
    this.modeDisplacementTextures = [];
    this.state.modes = modes;

    modes.forEach(mode => {
      const disps = mode.modal_displacements || [];
      const res = Math.ceil(Math.sqrt(disps.length));
      const size = Math.max(res, 16);

      const data = new Float32Array(size * size * 4);
      let maxW = 0;
      disps.forEach(d => { if (Math.abs(d[2]) > maxW) maxW = Math.abs(d[2]); });
      if (maxW < 1e-6) maxW = 1;

      for (let y = 0; y < size; y++) {
        for (let x = 0; x < size; x++) {
          const idx = y * size + x;
          const di = disps[idx] ? disps[idx][2] : 0;
          const norm = (di / maxW + 1) * 0.5;
          data[idx * 4] = norm;
          data[idx * 4 + 1] = norm;
          data[idx * 4 + 2] = norm;
          data[idx * 4 + 3] = 1;
        }
      }

      const tex = new THREE.DataTexture(data, size, size, THREE.RGBAFormat, THREE.FloatType);
      tex.needsUpdate = true;
      tex.minFilter = THREE.LinearFilter;
      tex.magFilter = THREE.LinearFilter;
      tex.wrapS = THREE.ClampToEdgeWrapping;
      tex.wrapT = THREE.ClampToEdgeWrapping;

      this.modeDisplacementTextures.push({
        texture: tex,
        frequency: mode.frequency_hz,
        amplitude: 0.03 * (1 / (1 + mode.mode_order * 0.3)),
        nonlinearFreq: mode.nonlinear_frequency_hz || mode.frequency_hz,
      });
    });
  }

  updateModeAnimationGPU(timeSec) {
    if (!this.modeShaderMaterial || this.state.currentView !== 'modes') return;

    const modeData = this.modeDisplacementTextures[this.state.currentModeIndex];
    if (!modeData) return;

    const u = this.modeShaderMaterial.uniforms;
    u.uTime.value = timeSec;
    u.uDisplacementTex.value = modeData.texture;
    u.uFrequency.value = modeData.nonlinearFreq;
    u.uAmplitude.value = modeData.amplitude;
    u.uUseModeColors.value = 1.0;
  }

  resetModeAnimationGPU() {
    if (!this.modeShaderMaterial) return;
    this.modeShaderMaterial.uniforms.uUseModeColors.value = 0.0;
    this.modeShaderMaterial.uniforms.uAmplitude.value = 0.0;
  }

  buildBronzeDrum(diameterCm = 78.5, heightCm = 52.3) {
    while (this.drumGroup.children.length) {
      const c = this.drumGroup.children[0];
      if (c.geometry) c.geometry.dispose();
      if (c.material) c.material.dispose();
      this.drumGroup.remove(c);
    }
    this.frogMeshes = [];
    this.defectMarkers = [];
    if (this.soundFieldGroup) {
      while (this.soundFieldGroup.children.length) this.soundFieldGroup.remove(this.soundFieldGroup.children[0]);
      this.scene.remove(this.soundFieldGroup);
      this.soundFieldGroup = null;
    }

    const scale = 0.025;
    const radius = diameterCm * scale / 2;
    const height = heightCm * scale;
    const thickness = radius * 0.05;
    this.radius = radius;

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

    if (this.modeShaderMaterial) this.modeShaderMaterial.dispose();
    this.modeShaderMaterial = this.createModeShaderMaterial(bronzeMaterial);
    this.modeShaderMaterial.uniforms.uRadius.value = radius;
    this.drumMesh = new THREE.Mesh(faceGeom, this.modeShaderMaterial);
    this.drumMesh.userData.radius = radius;
    this.drumGroup.add(this.drumMesh);

    const rimGeom = new THREE.TorusGeometry(radius - thickness * 0.3, thickness * 0.7, 16, 96);
    this.rimMesh = new THREE.Mesh(rimGeom, new THREE.MeshStandardMaterial({
      color: 0x9a6a08, metalness: 0.9, roughness: 0.25
    }));
    this.rimMesh.position.y = thickness * 0.2;
    this.drumGroup.add(this.rimMesh);

    const sidePts = [];
    const segments = 96;
    for (let i = 0; i <= segments; i++) {
      const t = i / segments;
      const waistT = Math.sin(t * Math.PI);
      const localR = radius * (1.02 - 0.12 * waistT) * (1 - 0.08 * t * t);
      sidePts.push(new THREE.Vector2(localR, -height + t * height));
    }
    const sideGeom = new THREE.LatheGeometry(sidePts, 96);
    this.sideMesh = new THREE.Mesh(sideGeom, bronzeMaterial.clone());
    this.sideMesh.position.y = thickness * 0.1;
    this.drumGroup.add(this.sideMesh);

    this.buildSunRays(radius, thickness);
    this.buildHalos(radius, thickness);
    this.buildFrogs(radius, thickness);
    this.buildEars(radius, height, thickness);
    this.buildFoot(radius, height, thickness);

    this.controls.target.set(0, -height * 0.2, 0);
    return { radius, height, thickness };
  }

  buildSunRays(radius, thickness) {
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
    this.drumGroup.add(sunRays);
  }

  buildHalos(radius, thickness) {
    for (let ring = 0; ring < 3; ring++) {
      const rr = radius * (0.35 + ring * 0.2);
      const torus = new THREE.Mesh(
        new THREE.TorusGeometry(rr, thickness * 0.04, 8, 96),
        new THREE.MeshStandardMaterial({ color: 0xa87506, metalness: 0.9, roughness: 0.3 })
      );
      torus.position.y = thickness * 0.45;
      torus.rotation.x = Math.PI / 2;
      this.drumGroup.add(torus);
    }
  }

  buildFrogs(radius, thickness) {
    for (let f = 0; f < 4; f++) {
      const angle = (f / 4) * Math.PI * 2 + Math.PI / 4;
      const frog = this.createFrog(thickness);
      frog.position.set(Math.cos(angle) * radius * 0.85, thickness * 0.4, Math.sin(angle) * radius * 0.85);
      frog.rotation.y = -angle;
      this.drumGroup.add(frog);
      this.frogMeshes.push(frog);
    }
  }

  createFrog(thickness) {
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

  buildEars(radius, height, thickness) {
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
      this.drumGroup.add(earGroup);
    }
  }

  buildFoot(radius, height, thickness) {
    const foot = new THREE.Mesh(
      new THREE.CylinderGeometry(radius * 1.02, radius * 1.08, thickness * 1.2, 96),
      new THREE.MeshStandardMaterial({ color: 0x7a5600, metalness: 0.88, roughness: 0.35 })
    );
    foot.position.y = -height - thickness * 0.5;
    this.drumGroup.add(foot);
  }

  switchView(viewName) {
    this.state.currentView = viewName;
    if (this.soundFieldGroup) this.soundFieldGroup.visible = viewName === 'soundfield';
    this.defectMarkers.forEach(m => m.visible = viewName === 'defects');

    if (this.drumMesh) this.drumMesh.visible = true;

    if (viewName === 'soundfield' && (!this.soundFieldGroup || !this.soundFieldGroup.children.length)) {
      this.buildSoundField();
    }
    if (viewName === 'defects' && this.state.defects.length && !this.defectMarkers.length) {
      this.buildDefectMarkers();
    }
  }

  updateModeAnimation(timeSec) {
    this.updateModeAnimationGPU(timeSec);

    if (this.state.currentView === 'modes' && this.frogMeshes.length) {
      const modeData = this.modeDisplacementTextures[this.state.currentModeIndex];
      if (modeData) {
        const phase = timeSec * modeData.nonlinearFreq * Math.PI * 2;
        const amp = modeData.amplitude;
        this.frogMeshes.forEach((f, idx) => {
          f.position.y = (f.userData.baseY = f.userData.baseY ?? f.position.y) + Math.sin(phase + idx) * amp * 0.5;
        });
      }
    }
  }

  resetModeDeformation() {
    this.resetModeAnimationGPU();
    this.frogMeshes.forEach(f => { if (f.userData.baseY != null) f.position.y = f.userData.baseY; });
  }

  buildSoundField() {
    if (!this.state.soundField.length) return;
    if (this.soundFieldGroup) this.scene.remove(this.soundFieldGroup);
    this.soundFieldGroup = new THREE.Group();
    const points = this.state.soundField;
    let maxSpl = 0, minSpl = Infinity;
    points.forEach(p => { maxSpl = Math.max(maxSpl, p.spl_db); minSpl = Math.min(minSpl, p.spl_db); });
    const range = Math.max(1, maxSpl - minSpl);
    const glowTex = this.makeGlowTexture();

    points.forEach(p => {
      const norm = (p.spl_db - minSpl) / range;
      const col = new THREE.Color().setHSL(0.65 - norm * 0.65, 0.9, 0.55);
      const size = 0.06 + norm * 0.14;
      const mat = new THREE.SpriteMaterial({ map: glowTex, transparent: true, depthWrite: false, blending: THREE.AdditiveBlending, color: col, opacity: 0.75 });
      const sprite = new THREE.Sprite(mat);
      sprite.position.set((p.x_m - 0.5) * 10, (p.z_m - 0.5) * 10 + 1.5, (p.y_m - 0.5) * 10);
      sprite.scale.setScalar(size * 3);
      this.soundFieldGroup.add(sprite);
    });

    const shell = new THREE.Mesh(
      new THREE.SphereGeometry(3.5, 32, 16, 0, Math.PI * 2, 0, Math.PI / 2),
      new THREE.MeshBasicMaterial({ color: 0x3b82f6, transparent: true, opacity: 0.04, wireframe: true })
    );
    this.soundFieldGroup.add(shell);
    this.scene.add(this.soundFieldGroup);
    this.soundFieldGroup.visible = this.state.currentView === 'soundfield';
  }

  makeGlowTexture() {
    if (this.glowTexCache) return this.glowTexCache;
    const c = document.createElement('canvas');
    c.width = c.height = 64;
    const g = c.getContext('2d');
    const grd = g.createRadialGradient(32, 32, 0, 32, 32, 32);
    grd.addColorStop(0, 'rgba(255,255,255,1)');
    grd.addColorStop(0.3, 'rgba(255,255,255,0.6)');
    grd.addColorStop(1, 'rgba(255,255,255,0)');
    g.fillStyle = grd; g.fillRect(0, 0, 64, 64);
    this.glowTexCache = new THREE.CanvasTexture(c);
    return this.glowTexCache;
  }

  setDefects(defects) {
    this.state.defects = defects || [];
    this.defectMarkers = [];
  }

  setSoundField(field) {
    this.state.soundField = field || [];
  }

  buildDefectMarkers() {
    if (!this.drumMesh || !this.state.defects.length) return;
    const R = this.drumMesh.geometry.parameters?.radiusTop || 1;
    this.state.defects.forEach(def => {
      const col = this.sevColor(def.severity);
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
      this.drumGroup.add(g);
      this.defectMarkers.push(g);
      g.visible = this.state.currentView === 'defects';
    });
  }

  sevColor(sev) {
    if (sev >= 0.85) return 0x991b1b;
    if (sev >= 0.7) return 0xef4444;
    if (sev >= 0.5) return 0xf97316;
    if (sev >= 0.3) return 0xeab308;
    return 0x22c55e;
  }

  updateDefectPulse(t) {
    if (this.state.currentView !== 'defects') return;
    this.defectMarkers.forEach(g => {
      g.children.forEach(c => {
        if (c.userData.isPulse) {
          const s = 1 + 0.6 * Math.sin(t * 4 + g.position.x);
          c.scale.setScalar(s);
          c.material.opacity = 0.5 * (1 - 0.5 * Math.abs(Math.sin(t * 4)));
        }
      });
    });
  }

  animate = () => {
    requestAnimationFrame(this.animate);
    this.state.time = (performance.now() - this.startTime) / 1000;
    if (this.state.animating) {
      if (this.state.currentView === 'modes') this.updateModeAnimation(this.state.time);
      else if (this.state.currentView !== 'modes') this.resetModeDeformation();
      if (this.state.currentView === 'defects') this.updateDefectPulse(this.state.time);
    } else {
      this.resetModeDeformation();
    }
    this.controls.update();
    this.renderer.render(this.scene, this.camera);
  };

  onResize() {
    this.renderer.setSize(this.canvas.clientWidth, this.canvas.clientHeight, false);
    this.camera.aspect = this.canvas.clientWidth / this.canvas.clientHeight;
    this.camera.updateProjectionMatrix();
  }

  setModeIndex(idx) {
    this.state.currentModeIndex = idx;
  }

  setAnimating(flag) {
    this.state.animating = flag;
  }
}
