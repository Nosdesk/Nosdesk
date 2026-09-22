<!-- ErrorView.vue -->
<script setup lang="ts">
/**
 * The error code as a still frame on a CRT that has lost its lock: the glyphs
 * are torn by a two-dimensional turbulence field, split into three colour
 * channels, and drawn to the pointer like the filament of a plasma globe.
 * Everything is a pure function of time in seconds and the pointer state, so
 * 60 Hz and 120 Hz displays show the same motion and nothing re-rolls.
 *
 * Per channel c in {-1, 0, +1} (dark: R G B, light: C M Y) at CSS px p:
 *
 *   d_c = sH E [ Ab Nb + Af Nf + gBand Ag Ng ] + sH E c (S + g s Sg) + pull_c
 *
 * Nb is a one-octave row field (frequency 1:10, the horizontal tear bands),
 * Nf a three-octave fine field, Ng the field of a scheduled glitch event;
 * all are |noise| turbulence, translated over time, never re-rolled, with an
 * explicit 8-bit premultiply term (J) that reproduces the hairline bristles
 * the original SVG filter produced by accident. E is the hover hold. The
 * pull anchors the glyph edge nearest the pointer so it lands exactly under
 * the cursor, with the tear texture concentrated on that row and carried
 * with the pointer. Glitch events are decided by a hash of the time window,
 * so they are discrete and deterministic. Scanline luminance is a comb on
 * the output row. Dark themes add the three channels, light themes multiply.
 */
import Button from '@/components/common/Button.vue'
import { useRoute, useRouter } from 'vue-router'
import { performBack } from '@/router/navigation'
import { onMounted, onBeforeUnmount, ref, reactive, computed, watch, useTemplateRef } from 'vue'
import { useFluent } from 'fluent-vue'
import { useThemeStore } from '@/stores/theme'
import { useReducedMotion } from '@/composables/useReducedMotion'
import ToggleSwitch from '@/components/common/ToggleSwitch.vue'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const route = useRoute()
const router = useRouter()
const goBack = () => performBack(router, route)
const goHome = () => router.push('/')

const themeStore = useThemeStore()
const isDarkMode = computed(() => themeStore.isDarkMode)
const isEpaperTheme = computed(() => themeStore.effectiveTheme?.meta?.id === 'epaper')
const reduced = useReducedMotion()
const isStatic = computed(() => reduced.value || isEpaperTheme.value)
const fallback = ref(false)

const canvasRef = useTemplateRef<HTMLCanvasElement>('mark')

const markWidth = ref('60rem')
const markHeight = ref('24rem')
const fontSize = ref('14rem')
const errorCode = ref(t('error-page-default-code'))
const errorMessage = ref(t('error-page-default-message'))

// --- Parameters ---
// px amplitudes are for a 400 px tall mark and scale with its height (sH).
// Spatial frequencies are cycles per CSS px at that height, rates are Hz,
// radii are fractions of the mark height, times are seconds.
const H_REF = 400
const P = reactive({
  enabled: true,
  smooth: false,
  Ab: 5, // band field amplitude, px
  Kb: [0.001, 0.01], // band field frequency
  Db: [12, 32], // band field channel offset, px
  Af: 10, // fine field amplitude, px
  Kf: [0.004, 0.0009], // fine field frequency
  Df: [60, 30], // fine field channel offset, px
  octF: 3, // fine field octaves
  S: [1.25, 0.6], // constant channel split, px
  J: 1, // bristle gain
  Ka: [0.03, 0.003], // bristle field frequencies
  Ka2: [0.003, 0.03],
  vb: 12, // band field drift, px/s
  vf: 25, // fine field drift, px/s
  Wb: [160, 40], // band sway amplitude, px
  Pb: [29, 12], // band sway period, s
  Wa: 8, // bristle sway amplitude, px
  Pa: [13, 17], // bristle sway period, s
  sigmaR: 0.12, // hold radius
  tauIn: 0.12, // hold ease in, s
  tauOut: 1.2, // hold release, s
  tauP: 0.07, // pointer lag, s
  Wg: 6, // glitch window, s
  Pg: 0.95, // glitch chance per window
  tauG: 0.45, // glitch length, s
  rIn: 0.04, // glitch attack, s
  rOut: 0.16, // glitch release, s
  tauStep: 0.09, // glitch hold, s
  vg: 300, // glitch field speed, px/s
  Pcol: 0.7, // share of column glitches
  Ag: 23.5, // glitch amplitude, px
  Kg: [0.001, 0.01], // glitch field frequency
  sigmaG: 0.25, // glitch band half-width
  Sg: [5, 4], // glitch channel jump, px
  Ap: 18, // pull tear amplitude, px
  Sp: 6, // pull split, px
  sigX: 0.175, // pull window
  sigY: 0.045, // pull band half-width
  sigV: 0.14, // pinch window
  kY: 0.3, // pinch
  wTex: 0.35, // pull texture floor across the row
  dMax: [0.3, 0.15], // pull reach clamp
  box: [0.35, 0.25], // edge search box
  lam: 1.5, // same-row preference in the edge search
  tauE: 0.05, // edge anchor lag, s
  tauPin: 0.12, // pull strike, s
  tauPout: 0.3, // pull release, s
  aT: 0.05, // tap attack, s
  hT: 0.15, // tap hold, s
  tauD: 0.35, // tap decay, s
  tauS: 0.2, // click surge decay, s
  aPhi: 0.35, // pull flicker depth
  fPhi: [9.1, 13.7], // pull flicker, Hz
  vP: 120, // pull field stream, px/s
  tauPs: 0.25, // pull field re-roll, s
  ps: 2, // scanline pitch, CSS px
  as: 0.2, // scanline depth
  ab: 0.06, // trough depth
  sigmaB: 0.06, // trough half-width
  fb: 0.08, // trough roll rate, Hz
})

// Debug sliders (press d). Labels are the parameter symbols, locale-invariant.
const showDebug = ref(false)
const debugSliders: Array<{ key: string; label: string; min: number; max: number; step: number }> = [
  { key: 'Ab', label: 'Ab band px', min: 0, max: 24, step: 0.5 },
  { key: 'Af', label: 'Af fine px', min: 0, max: 16, step: 0.25 },
  { key: 'Db.1', label: 'Db,y channel offset px', min: 0, max: 60, step: 1 },
  { key: 'S.0', label: 'S split px', min: 0, max: 8, step: 0.25 },
  { key: 'J', label: 'J bristles', min: 0, max: 2, step: 0.05 },
  { key: 'vb', label: 'vb band drift px/s', min: 0, max: 60, step: 1 },
  { key: 'vf', label: 'vf fine drift px/s', min: 0, max: 120, step: 1 },
  { key: 'Ag', label: 'Ag glitch px', min: 0, max: 40, step: 0.5 },
  { key: 'Pg', label: 'Pg glitch chance', min: 0, max: 1, step: 0.05 },
  { key: 'tauG', label: 'tauG glitch length s', min: 0.1, max: 1.5, step: 0.05 },
  { key: 'Ap', label: 'Ap pull tear px', min: 0, max: 40, step: 0.5 },
  { key: 'Sp', label: 'Sp pull split px', min: 0, max: 12, step: 0.25 },
  { key: 'sigY', label: 'sigmaY band half-width', min: 0.02, max: 0.15, step: 0.005 },
  { key: 'kY', label: 'kY pinch', min: 0, max: 1, step: 0.05 },
  { key: 'as', label: 'as scanline depth', min: 0, max: 0.5, step: 0.01 },
]
const params = P as unknown as Record<string, number | number[]>
const getP = (k: string): number => {
  const [a, b] = k.split('.')
  const v = params[a]
  return Array.isArray(v) ? v[Number(b)] : v
}
const setP = (k: string, value: number): void => {
  const [a, b] = k.split('.')
  const v = params[a]
  if (Array.isArray(v)) v[Number(b)] = value
  else params[a] = value
}
const stats = ref('')
const handleKeydown = (e: KeyboardEvent) => {
  if ((e.key === 'd' || e.key === 'D') && !(e.target instanceof HTMLInputElement)) showDebug.value = !showDebug.value
}

// --- Shader ---
const VERT = '#version 300 es\nvoid main(){vec2 v=vec2((gl_VertexID<<1)&2,gl_VertexID&2);gl_Position=vec4(v*2.0-1.0,0,1);}'
const FRAG = `#version 300 es
precision highp float;
uniform sampler2D u_text;
uniform vec2  u_res;
uniform float u_dpr;
uniform float u_sH;
uniform float u_lightMode;
uniform vec3  u_bg;
uniform float u_Ab, u_Af, u_J, u_octF;
uniform vec2  u_Kb, u_Kf, u_Ka, u_Ka2, u_Db, u_Df, u_S;
uniform vec2  u_wb, u_wf, u_wa;
uniform vec2  u_ptr;
uniform float u_hover, u_sigmaR;
// pull: the tear drawn to the pointer (see 404 pull design)
uniform float u_pull, u_kY, u_sigV, u_sigT, u_Ap, u_Sp, u_oP, u_wTex;
uniform vec2  u_dP, u_sigP, u_Kp;
uniform float u_g, u_Ag, u_sign, u_ySig, u_yK, u_oK, u_colG;
uniform vec2  u_Kg, u_Sg;
uniform float u_pitch, u_as, u_ab, u_yb, u_sigB;
out vec4 o;

float hash(vec2 p, float s) {
  p = fract(p * vec2(0.1031, 0.1030) + s);
  p += dot(p, p.yx + 33.33);
  return fract((p.x + p.y) * p.x);
}
// Perlin gradient noise with the SVG spec's s-curve, peaks near 1.
float gnoise(vec2 q, float s) {
  vec2 i = floor(q), f = q - i;
  vec2 u = f * f * (3.0 - 2.0 * f);
  float a = 6.2831853 * hash(i, s),              b = 6.2831853 * hash(i + vec2(1, 0), s);
  float c = 6.2831853 * hash(i + vec2(0, 1), s), d = 6.2831853 * hash(i + vec2(1, 1), s);
  float n = mix(mix(dot(vec2(cos(a), sin(a)), f),              dot(vec2(cos(b), sin(b)), f - vec2(1, 0)), u.x),
                mix(dot(vec2(cos(c), sin(c)), f - vec2(0, 1)), dot(vec2(cos(d), sin(d)), f - vec2(1, 1)), u.x), u.y);
  return n * 1.6;
}
// Turbulence: sum of |noise|, lacunarity 2, gain 0.5, normalised to [0, 1].
float turb(vec2 q, float s, float octaves) {
  float v = 0.0, norm = 0.0, amp = 1.0;
  for (int k = 0; k < 3; k++) {
    if (float(k) >= octaves) break;
    v += amp * abs(gnoise(q * exp2(float(k)) + 17.3 * float(k), s + 101.0 * float(k)));
    norm += amp; amp *= 0.5;
  }
  return v / norm;
}
const float MU1 = 0.274, MU3 = 0.273;
float q8(float v) { return floor(v * 255.0 + 0.5) / 255.0; }
// The 8-bit premultiplied round trip of the original filter: the bristles.
float unpre(float R, float A) { return R + u_J * (q8(R * A) / max(q8(A), 1.0 / 255.0) - R); }
float cov(vec2 pd) { return texture(u_text, pd / u_res).r; }

void main() {
  vec2 pd = vec2(gl_FragCoord.x, u_res.y - gl_FragCoord.y);
  vec2 p = pd / u_dpr;
  float sH = u_sH;
  float Hcss = u_res.y / u_dpr;

  vec2 dp = p - u_ptr;
  float E  = 1.0 - u_hover * exp(-dot(dp, dp) / (2.0 * u_sigmaR * u_sigmaR));
  float Gy = exp(-dp.y * dp.y / (2.0 * u_sigP.y * u_sigP.y));
  float Gt = exp(-dp.y * dp.y / (2.0 * u_sigT * u_sigT));
  float Wx = exp(-dp.x * dp.x / (2.0 * u_sigP.x * u_sigP.x));
  float Gv = exp(-dp.y * dp.y / (2.0 * u_sigV * u_sigV));
  // the curve: the nearest glyph edge is shifted so it lands on the pointer, rows funnel toward the pointer row
  vec2  dPull = u_pull * (u_dP * Gy * Wx - vec2(0.0, dp.y * u_kY * Gv * Wx));
  float tex = u_pull * Gt * mix(u_wTex, 1.0, Wx);
  float yN = p.y / Hcss;
  float gBand = u_g * exp(-(yN - u_yK) * (yN - u_yK) / (2.0 * u_ySig * u_ySig));

  vec3 tap = vec3(0.0);
  for (int i = 0; i < 3; i++) {
    float c = float(i) - 1.0;
    vec2 qb  = u_Kb  / sH * (p + u_wb + c * u_Db * sH);
    vec2 qf  = u_Kf  / sH * (p + u_wf + c * u_Df * sH);
    vec2 qa  = u_Ka  / sH * (p + u_wa + c * u_Db * sH);
    vec2 qa2 = u_Ka2 / sH * (p + u_wa + c * u_Df * sH);
    float Ab = turb(qa + vec2(3.7, 1.3), 53.0, 1.0);
    float Af = turb(qa2 + vec2(2.1, 4.4), 59.0, 1.0);
    vec2 Nb = (vec2(unpre(turb(qb, 11.0, 1.0), Ab), unpre(turb(qb, 23.0, 1.0), Ab)) - MU1) / (1.0 - MU1);
    vec2 Nf = (vec2(unpre(turb(qf, 37.0, u_octF), Af), unpre(turb(qf, 41.0, u_octF), Af)) - MU3) / (1.0 - MU3);
    vec2 d = u_Ab * Nb + u_Af * Nf + c * u_S;
    if (u_g > 0.0) {
      vec2 qg = u_Kg / sH * (p + mix(vec2(0.0, u_oK), vec2(u_oK, 0.0), u_colG) * sH + c * u_Db * sH);
      vec2 Ng = (vec2(turb(qg, 61.0, 1.0), turb(qg, 67.0, 1.0)) - MU1) / (1.0 - MU1);
      d += gBand * u_Ag * Ng + u_g * u_sign * c * u_Sg;
    }
    d *= sH * E;
    if (u_pull > 0.0) {
      vec2 qp = u_Kp / sH * (p - u_ptr + vec2(0.0, u_oP) + c * u_Db * sH);
      vec2 Np = (vec2(turb(qp, 71.0, 1.0), turb(qp, 73.0, 1.0)) - MU1) / (1.0 - MU1);
      d += dPull + tex * sH * (u_Ap * Np + c * vec2(u_Sp, 0.4 * u_Sp));
    }
    float v = cov(pd - d * u_dpr);
    if (i == 0) tap.x = v; else if (i == 1) tap.y = v; else tap.z = v;
  }

  float L = 1.0 - u_as * (0.5 + 0.5 * cos(6.2831853 * pd.y / u_pitch));
  float B = 1.0 - u_ab * exp(-(yN - u_yb) * (yN - u_yb) / (2.0 * u_sigB * u_sigB));
  tap *= L * B;

  vec3 col = u_lightMode > 0.5
    ? u_bg * (1.0 - tap.x * vec3(1, 0, 0)) * (1.0 - tap.y * vec3(0, 1, 0)) * (1.0 - tap.z * vec3(0, 0, 1))
    : u_bg + tap;
  o = vec4(clamp(col, 0.0, 1.0), 1.0);
}`
const UNIFORMS = [
  'u_text', 'u_res', 'u_dpr', 'u_sH', 'u_lightMode', 'u_bg', 'u_Ab', 'u_Af', 'u_J', 'u_octF',
  'u_Kb', 'u_Kf', 'u_Ka', 'u_Ka2', 'u_Db', 'u_Df', 'u_S', 'u_wb', 'u_wf', 'u_wa',
  'u_ptr', 'u_hover', 'u_sigmaR', 'u_pull', 'u_dP', 'u_sigP', 'u_sigT', 'u_sigV', 'u_kY', 'u_Ap', 'u_Sp', 'u_oP', 'u_wTex', 'u_Kp',
  'u_g', 'u_Ag', 'u_sign', 'u_ySig', 'u_yK', 'u_oK', 'u_colG', 'u_Kg', 'u_Sg',
  'u_pitch', 'u_as', 'u_ab', 'u_yb', 'u_sigB',
] as const
type UniformName = (typeof UNIFORMS)[number]
type Uniforms = Record<UniformName, WebGLUniformLocation | null>

// --- Deterministic helpers ---
// Per-window hash in [0, 1): mulberry32 on (k, salt).
const hashU = (k: number, salt: number): number => {
  let a = (Math.imul(k, 0x9e3779b1) ^ Math.imul(salt + 1, 0x85ebca6b)) >>> 0
  a = (a + 0x6d2b79f5) >>> 0
  let z = Math.imul(a ^ (a >>> 15), 1 | a)
  z = (z + Math.imul(z ^ (z >>> 7), 61 | z)) ^ z
  return ((z ^ (z >>> 14)) >>> 0) / 4294967296
}
const ramp = (x: number) => 0.5 - 0.5 * Math.cos(Math.PI * Math.min(Math.max(x, 0), 1))

// Glitch state for time t: a hash of the window decides whether, when, where
// and in which orientation a burst happens.
interface Glitch { g: number; yK: number; sign: number; oK: number; col: boolean }
const NO_GLITCH: Glitch = { g: 0, yK: 0.5, sign: 1, oK: 0, col: false }
const glitchAt = (time: number): Glitch => {
  const k = Math.floor(time / P.Wg)
  if (hashU(k, 0) >= P.Pg) return NO_GLITCH
  const start = k * P.Wg + hashU(k, 1) * (P.Wg - P.tauG)
  if (time < start || time > start + P.tauG) return NO_GLITCH
  const s = time - start
  const g = Math.min(ramp(s / P.rIn), ramp((P.tauG - s) / P.rOut))
  const step = Math.floor(s / P.tauStep)
  return {
    g,
    yK: hashU(k, 4),
    sign: hashU(k, 3) < 0.5 ? -1 : 1,
    col: hashU(k, 5) < P.Pcol,
    oK: (hashU(k, 2) + hashU(k, 10 + step)) * 1000 + P.vg * s,
  }
}

// Pointer state, CSS px on the mark, y down. Only h, ptr, pE and pull are
// smoothed (clamped-dt exponentials); everything else is closed-form in t.
const st = {
  h: 0,
  hTarget: 0,
  ptr: [0, 0],
  ptrTarget: [0, 0],
  pull: 0,
  pullTap: 0,
  pE: [0, 0],
  pEValid: false,
  tapT: -1e9,
  relT: -1e9,
  held: false,
  surgeT: -1e9,
}

// --- Rasterised text ---
interface Edges { W: number; H: number; starts: Uint32Array; xs: Uint16Array }
let gl: WebGL2RenderingContext | null = null
let tex: WebGLTexture | null = null
let U: Uniforms | null = null
let texDpr = 0
let edges: Edges | null = null
let rasterGen = 0

const currentDpr = () => Math.min(window.devicePixelRatio || 1, 2)
const bgRgb = (): Float32Array => {
  const m = getComputedStyle(document.documentElement).getPropertyValue('--color-app').trim().match(/^#([0-9a-f]{6})$/i)
  const n = parseInt(m ? m[1] : '000000', 16)
  return Float32Array.from([(n >> 16) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255])
}

const setFilter = (smooth: boolean) => {
  if (!gl) return
  const f = smooth ? gl.LINEAR : gl.NEAREST
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, f)
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, f)
}

const initGL = (canvas: HTMLCanvasElement): boolean => {
  const ctx = canvas.getContext('webgl2', { alpha: false, antialias: false })
  if (!ctx) return false
  const compile = (type: number, src: string) => {
    const s = ctx.createShader(type)
    if (!s) throw new Error('shader')
    ctx.shaderSource(s, src)
    ctx.compileShader(s)
    if (!ctx.getShaderParameter(s, ctx.COMPILE_STATUS)) throw new Error(ctx.getShaderInfoLog(s) ?? 'shader')
    return s
  }
  try {
    const prog = ctx.createProgram()
    if (!prog) throw new Error('program')
    ctx.attachShader(prog, compile(ctx.VERTEX_SHADER, VERT))
    ctx.attachShader(prog, compile(ctx.FRAGMENT_SHADER, FRAG))
    ctx.linkProgram(prog)
    if (!ctx.getProgramParameter(prog, ctx.LINK_STATUS)) throw new Error(ctx.getProgramInfoLog(prog) ?? 'link')
    ctx.useProgram(prog)
    U = Object.fromEntries(UNIFORMS.map((n) => [n, ctx.getUniformLocation(prog, n)])) as Uniforms
    tex = ctx.createTexture()
    ctx.bindTexture(ctx.TEXTURE_2D, tex)
    ctx.texParameteri(ctx.TEXTURE_2D, ctx.TEXTURE_WRAP_S, ctx.CLAMP_TO_EDGE)
    ctx.texParameteri(ctx.TEXTURE_2D, ctx.TEXTURE_WRAP_T, ctx.CLAMP_TO_EDGE)
    gl = ctx
    setFilter(P.smooth)
    ctx.uniform1i(U.u_text, 0)
    return true
  } catch (err) {
    console.warn('error mark: WebGL2 unavailable, drawing plain text', err)
    gl = null
    return false
  }
}

const raster = async (canvas: HTMLCanvasElement): Promise<void> => {
  const gen = ++rasterGen
  const dpr = currentDpr()
  const W = Math.round(canvas.clientWidth * dpr)
  const H = Math.round(canvas.clientHeight * dpr)
  if (!gl || W < 2 || H < 2) return
  const family = getComputedStyle(document.documentElement).getPropertyValue('--font-sans') || 'sans-serif'
  const font = `bold ${parseFloat(fontSize.value) * dpr}px ${family}`
  await document.fonts.load(font).catch(() => undefined)
  if (gen !== rasterGen || !gl) return
  // White on opaque black, so .r is real coverage at edge texels.
  const m = document.createElement('canvas')
  m.width = W
  m.height = H
  const c = m.getContext('2d')
  if (!c) return
  c.fillStyle = '#000'
  c.fillRect(0, 0, W, H)
  c.font = font
  c.textAlign = 'center'
  c.textBaseline = 'middle'
  c.fillStyle = '#fff'
  c.fillText(errorCode.value, W / 2, H / 2)
  // Per-row table of glyph edge crossings, for the pull's edge anchor.
  const img = c.getImageData(0, 0, W, H).data
  const starts = new Uint32Array(H + 1)
  const xs: number[] = []
  for (let r = 0; r < H; r += 1) {
    starts[r] = xs.length
    let prev = 0
    for (let x = 0; x < W; x += 1) {
      const on = img[(r * W + x) * 4] > 127 ? 1 : 0
      if (on !== prev) xs.push(x)
      prev = on
    }
  }
  starts[H] = xs.length
  edges = { W, H, starts, xs: Uint16Array.from(xs) }
  canvas.width = W
  canvas.height = H
  gl.viewport(0, 0, W, H)
  gl.bindTexture(gl.TEXTURE_2D, tex)
  gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, false)
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.R8, gl.RED, gl.UNSIGNED_BYTE, m)
  texDpr = dpr
}

// The glyph edge nearest the pointer, CSS px, or null when none is in the box.
const nearestEdge = (px: number, py: number): number[] | null => {
  if (!edges) return null
  const dpr = texDpr
  const Hc = edges.H / dpr
  const x0 = px * dpr
  const y0 = py * dpr
  const bx = P.box[0] * Hc * dpr
  const by = P.box[1] * Hc * dpr
  let best: number[] | null = null
  let bm = Infinity
  for (let r = Math.max(0, Math.ceil(y0 - by)); r <= Math.min(edges.H - 1, Math.floor(y0 + by)); r += 1) {
    let lo = edges.starts[r]
    let hi = edges.starts[r + 1]
    if (lo === hi) continue
    const end = hi
    while (hi - lo > 1) {
      const mid = (lo + hi) >> 1
      if (edges.xs[mid] <= x0) lo = mid
      else hi = mid
    }
    for (let i = lo; i <= lo + 1 && i < end; i += 1) {
      const dx = edges.xs[i] - x0
      const dy = r - y0
      if (Math.abs(dx) > bx) continue
      const mm = dx * dx + P.lam * P.lam * dy * dy
      if (mm < bm) {
        bm = mm
        best = [edges.xs[i] / dpr, r / dpr]
      }
    }
  }
  return best
}

const wander = (time: number) => ({
  wb: [P.Wb[0] * Math.sin((2 * Math.PI * time) / P.Pb[0]), P.vb * time + P.Wb[1] * Math.sin((2 * Math.PI * time) / P.Pb[1] + 1)],
  wf: [P.vf * time, 0],
  wa: [P.Wa * Math.sin((2 * Math.PI * time) / P.Pa[0]) + P.vb * 0.3 * time, P.Wa * Math.sin((2 * Math.PI * time) / P.Pa[1] + 2)],
})

// One frame. flat draws the converged text (epaper, effect off); otherwise the
// field at time t.
const draw = (canvas: HTMLCanvasElement, time: number, flat: boolean): void => {
  if (!gl || !U || !texDpr) return
  const W = canvas.width
  const H = canvas.height
  const dpr = texDpr
  const sH = H / dpr / H_REF
  const Hc = H / dpr
  const f2 = (a: number[]) => Float32Array.from(a)
  const { wb, wf, wa } = wander(time)
  const ev = flat ? NO_GLITCH : glitchAt(time)
  const amp = flat ? 0 : 1
  const pull = flat ? 0 : Math.max(st.pull, st.pullTap)
  const Phi = 1 + P.aPhi * (0.6 * Math.sin(2 * Math.PI * P.fPhi[0] * time) + 0.4 * Math.sin(2 * Math.PI * P.fPhi[1] * time + 1))
  const Sig = 1 + Math.exp(-Math.max(0, time - st.surgeT) / P.tauS)
  const cl = (v: number, mx: number) => Math.max(-mx, Math.min(mx, v))
  const dP = [cl(st.ptr[0] - st.pE[0], P.dMax[0] * Hc), cl(st.ptr[1] - st.pE[1], P.dMax[1] * Hc)]
  const sigX = Math.max(P.sigX * Hc, 0.8 * Math.abs(dP[0]))
  const sigY = Math.max(P.sigY * Hc * (1 + 0.2 * (Phi - 1)), 0.8 * Math.abs(dP[1]))
  gl.uniform2f(U.u_res, W, H)
  gl.uniform1f(U.u_dpr, dpr)
  gl.uniform1f(U.u_sH, sH)
  gl.uniform1f(U.u_lightMode, isDarkMode.value ? 0 : 1)
  gl.uniform3fv(U.u_bg, bgRgb())
  gl.uniform1f(U.u_Ab, amp * P.Ab)
  gl.uniform1f(U.u_Af, amp * P.Af)
  gl.uniform1f(U.u_J, P.J)
  gl.uniform1f(U.u_octF, P.octF)
  gl.uniform2fv(U.u_Kb, f2(P.Kb))
  gl.uniform2fv(U.u_Kf, f2(P.Kf))
  gl.uniform2fv(U.u_Ka, f2(P.Ka))
  gl.uniform2fv(U.u_Ka2, f2(P.Ka2))
  gl.uniform2fv(U.u_Db, f2(P.Db))
  gl.uniform2fv(U.u_Df, f2(P.Df))
  gl.uniform2f(U.u_S, amp * P.S[0], amp * P.S[1])
  gl.uniform2fv(U.u_wb, f2(wb))
  gl.uniform2f(U.u_wf, wf[0] + wb[0] / 2, wf[1] + wb[1] / 2)
  gl.uniform2fv(U.u_wa, f2(wa))
  gl.uniform2fv(U.u_ptr, f2(st.ptr))
  gl.uniform1f(U.u_hover, flat ? 0 : Math.max(st.h, st.pullTap))
  gl.uniform1f(U.u_sigmaR, P.sigmaR * Hc)
  gl.uniform1f(U.u_pull, pull)
  gl.uniform2fv(U.u_dP, f2(dP))
  gl.uniform2f(U.u_sigP, sigX, sigY)
  gl.uniform1f(U.u_sigT, P.sigY * Hc)
  gl.uniform1f(U.u_sigV, P.sigV * Hc)
  gl.uniform1f(U.u_kY, P.kY)
  gl.uniform1f(U.u_wTex, P.wTex)
  gl.uniform1f(U.u_Ap, P.Ap * Phi * Sig)
  gl.uniform1f(U.u_Sp, P.Sp * Phi * Sig)
  gl.uniform1f(U.u_oP, 1000 * hashU(Math.floor(time / P.tauPs), 20) + P.vP * time)
  gl.uniform2fv(U.u_Kp, f2(P.Kg))
  gl.uniform1f(U.u_g, ev.g)
  gl.uniform1f(U.u_Ag, P.Ag)
  gl.uniform1f(U.u_sign, ev.sign)
  gl.uniform1f(U.u_ySig, ev.col ? 10 : P.sigmaG)
  gl.uniform1f(U.u_yK, ev.yK)
  gl.uniform1f(U.u_oK, ev.oK)
  gl.uniform1f(U.u_colG, ev.col ? 1 : 0)
  gl.uniform2fv(U.u_Kg, f2(ev.col ? [P.Kg[1], P.Kg[0]] : P.Kg))
  gl.uniform2fv(U.u_Sg, f2(ev.col ? [P.Sg[1], P.Sg[0]] : P.Sg))
  gl.uniform1f(U.u_pitch, Math.max(1, Math.round(P.ps * dpr)))
  gl.uniform1f(U.u_as, flat ? 0 : P.as)
  gl.uniform1f(U.u_ab, flat ? 0 : P.ab)
  gl.uniform1f(U.u_sigB, P.sigmaB)
  gl.uniform1f(U.u_yb, ((P.fb * time) % 1) * (1 + 6 * P.sigmaB) - 3 * P.sigmaB)
  gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4)
}

// --- Loop ---
// Field time is absolute; smoothing uses a clamped dt so 60 and 120 Hz agree.
// A small governor drops the fine field to two octaves, then the bristles,
// if more than a quarter of a second's frames run long.
let raf = 0
let t0 = 0
let last = 0
let fps = 0
let frameMs = 0
let tier = 0
let winStart = 0
let winLong = 0
let winN = 0
const frame = (now: number): void => {
  raf = 0
  const canvas = canvasRef.value
  if (!canvas) return
  const dt = Math.min((now - last) / 1000, 0.1)
  last = now
  if (dt > 0) {
    fps = fps * 0.95 + (1 / dt) * 0.05
    frameMs = frameMs * 0.9 + dt * 1000 * 0.1
  }
  const time = (now - t0) / 1000
  if (texDpr !== currentDpr()) void raster(canvas)

  st.h += (st.hTarget - st.h) * (1 - Math.exp(-dt / (st.hTarget > st.h ? P.tauIn : P.tauOut)))
  st.ptr[0] += (st.ptrTarget[0] - st.ptr[0]) * (1 - Math.exp(-dt / P.tauP))
  st.ptr[1] += (st.ptrTarget[1] - st.ptr[1]) * (1 - Math.exp(-dt / P.tauP))
  const tH = st.held ? Infinity : Math.max(st.relT, st.tapT + P.aT + P.hT)
  const pullTap = time < st.tapT ? 0 : ramp((time - st.tapT) / P.aT) * Math.exp(-Math.max(0, time - tH) / P.tauD)
  const e = st.hTarget > 0 || pullTap > 1e-3 ? nearestEdge(st.ptr[0], st.ptr[1]) : null
  if (e) {
    if (!st.pEValid) {
      st.pE = e
      st.pEValid = true
    } else {
      const k = 1 - Math.exp(-dt / P.tauE)
      st.pE[0] += (e[0] - st.pE[0]) * k
      st.pE[1] += (e[1] - st.pE[1]) * k
    }
  }
  const want = canHover && st.hTarget > 0 && e ? 1 : 0
  st.pull += (want - st.pull) * (1 - Math.exp(-dt / (want > st.pull ? P.tauPin : P.tauPout)))
  if (st.pull < 0.01 && pullTap < 0.01) st.pEValid = false
  st.pullTap = pullTap

  winN += 1
  if (dt > 0.02) winLong += 1
  if (now - winStart > 1000) {
    if (winN > 10 && winLong / winN > 0.25 && tier < 2) {
      tier += 1
      if (tier === 1) P.octF = 2
      if (tier === 2) P.J = 0
    }
    winStart = now
    winLong = 0
    winN = 0
  }
  if (showDebug.value) stats.value = `${fps.toFixed(0)} fps, ${frameMs.toFixed(1)} ms, tier ${tier}, ${canvas.width}x${canvas.height}`
  draw(canvas, time, !P.enabled)
  raf = requestAnimationFrame(frame)
}

const stop = () => {
  cancelAnimationFrame(raf)
  raf = 0
}
const start = () => {
  if (raf) return
  t0 = last = winStart = performance.now()
  raf = requestAnimationFrame(frame)
}
// Static themes draw one frame: reduced motion keeps the field at t = 0,
// epaper the converged text.
const drawStatic = (canvas: HTMLCanvasElement) => draw(canvas, 0, isEpaperTheme.value)

// --- Pointer ---
// CSS px on the mark, y down. Hover is gated on a hover-capable pointer, so
// touch gets tap and drag only.
const canHover = typeof window !== 'undefined' && window.matchMedia('(hover: hover)').matches
const at = (e: PointerEvent): number[] => {
  const r = (e.currentTarget as HTMLElement).getBoundingClientRect()
  return [e.clientX - r.left, e.clientY - r.top]
}
const nowS = () => (performance.now() - t0) / 1000
const enter = (e: PointerEvent) => {
  st.ptr = at(e)
  st.ptrTarget = at(e)
  st.hTarget = 1
}
const onEnter = (e: PointerEvent) => {
  if (canHover) enter(e)
}
// A pointer already resting on the mark when the route lands never crosses
// its boundary, so move also arms the hold.
const onMove = (e: PointerEvent) => {
  if (canHover) {
    if (st.hTarget === 0) enter(e)
    else st.ptrTarget = at(e)
  } else if (st.held) st.ptrTarget = at(e)
}
const onLeave = () => {
  st.hTarget = 0
}
const onDown = (e: PointerEvent) => {
  st.surgeT = nowS()
  if (canHover) return
  st.ptr = at(e)
  st.ptrTarget = at(e)
  st.pEValid = false
  st.tapT = nowS()
  st.held = true
}
const onRelease = () => {
  if (!st.held) return
  st.held = false
  st.relT = nowS()
}

// Size the mark from the viewport and the error code's length.
const adjustMarkSize = () => {
  const viewportWidth = window.innerWidth
  const textLength = errorCode.value.length
  const baseWidth = viewportWidth < 768 ? Math.min(viewportWidth * 0.9, 500) : Math.min(viewportWidth * 0.6, 1000)
  const widthAdjustment = textLength > 3 ? 1 + (textLength - 3) * 0.15 : 1
  const finalWidth = baseWidth * widthAdjustment
  markWidth.value = `${finalWidth}px`
  const aspectRatio = viewportWidth < 768 ? 2 : 2.5
  markHeight.value = `${finalWidth / aspectRatio}px`
  const maxFontPercentage = viewportWidth < 768 ? 0.6 : 0.7
  const calculatedFontSize = (finalWidth / aspectRatio) * maxFontPercentage
  const fontSizeAdjustment = textLength > 3 ? 1 / (1 + (textLength - 3) * 0.1) : 1
  fontSize.value = `${calculatedFontSize * fontSizeAdjustment}px`
}

let teardown: (() => void) | null = null

onMounted(async () => {
  errorCode.value = route.params.code?.toString() || t('error-page-default-code')
  errorMessage.value = route.params.message?.toString() || t('error-page-default-message')
  adjustMarkSize()
  window.addEventListener('resize', adjustMarkSize)
  window.addEventListener('keydown', handleKeydown)
  const canvas = canvasRef.value
  if (!canvas || !initGL(canvas)) {
    fallback.value = true
    return
  }
  await raster(canvas)
  const ro = new ResizeObserver(() => {
    void raster(canvas).then(() => {
      if (isStatic.value) drawStatic(canvas)
    })
  })
  ro.observe(canvas)
  const stopSmooth = watch(
    () => P.smooth,
    (s) => {
      if (gl) {
        gl.bindTexture(gl.TEXTURE_2D, tex)
        setFilter(s)
      }
    }
  )
  const stopTheme = watch(
    [isDarkMode, isStatic],
    () => {
      if (isStatic.value) {
        stop()
        drawStatic(canvas)
      } else start()
    },
    { immediate: true }
  )
  teardown = () => {
    stop()
    ro.disconnect()
    stopSmooth()
    stopTheme()
  }
})

onBeforeUnmount(() => {
  teardown?.()
  window.removeEventListener('resize', adjustMarkSize)
  window.removeEventListener('keydown', handleKeydown)
})
</script>

<template>
  <!-- Single root so App.vue's <Transition mode="out-in"> can attach
       leave/enter classes. -->
  <div class="h-full">
    <div class="error-page-container min-h-screen w-full flex items-center justify-center bg-app p-4 select-none">
      <div class="flex flex-col text-center">
        <div class="error-mark" :style="{ width: markWidth, height: markHeight }">
          <div
            v-if="fallback"
            class="flex h-full w-full items-center justify-center font-bold text-primary"
            :style="{ fontSize }"
            role="img"
            :aria-label="errorCode"
          >
            {{ errorCode }}
          </div>
          <canvas
            v-else
            ref="mark"
            role="img"
            :aria-label="errorCode"
            class="error-canvas block w-full h-full"
            @pointerenter="onEnter"
            @pointermove="onMove"
            @pointerleave="onLeave"
            @pointerdown="onDown"
            @pointerup="onRelease"
            @pointercancel="onRelease"
          />
        </div>
        <div class="flex flex-col gap-4">
          <div class="text-2xl text-secondary">
            {{ errorMessage }}
          </div>
          <p class="mt-2 text-tertiary">
            {{ $t('error-page-description') }}
          </p>
          <div class="mt-8 flex gap-4 justify-center">
            <button
              type="button"
              @click="goBack"
              class="px-4 py-2 text-sm font-medium text-secondary hover:text-primary transition-colors"
            >
              &larr; {{ $t('error-page-go-back') }}
            </button>
            <Button @click="goHome">
              {{ $t('error-page-go-home') }}
            </Button>
          </div>
        </div>
      </div>
    </div>
    <!-- Debug panel -->
    <div
      v-if="showDebug"
      class="fixed top-4 right-4 bg-surface/90 text-sm text-secondary p-4 rounded-lg max-h-[90vh] overflow-auto flex flex-col gap-3 z-overlay shadow-lg w-72"
    >
      <h3 class="font-semibold">{{ $t('error-page-debug-title') }}</h3>
      <ToggleSwitch v-model="P.enabled" :label="$t('error-page-debug-master-toggle')" size="sm" />
      <ToggleSwitch v-model="P.smooth" label="LINEAR taps" size="sm" />
      <div v-for="slider in debugSliders" :key="slider.key" class="flex flex-col gap-1">
        <label class="flex justify-between items-center gap-2">
          <span>{{ slider.label }}</span>
          <span class="tabular-nums w-14 text-right">{{ getP(slider.key).toFixed(3) }}</span>
        </label>
        <input
          type="range"
          :value="getP(slider.key)"
          :min="slider.min"
          :max="slider.max"
          :step="slider.step"
          class="w-full"
          @input="setP(slider.key, Number(($event.target as HTMLInputElement).value))"
        />
      </div>
      <div class="tabular-nums text-tertiary">{{ stats }}</div>
    </div>
  </div>
</template>

<style scoped>
.error-mark {
  max-width: 100%;
  transition: transform 0.2s ease-out;
}

.error-mark:hover {
  transform: scale(1.02);
}

.error-canvas {
  touch-action: pan-y pinch-zoom;
}

@keyframes float {
  0%,
  100% {
    transform: translateY(0px);
  }
  50% {
    transform: translateY(-10px);
  }
}

@media (prefers-reduced-motion: no-preference) {
  .error-mark {
    animation: float 6s ease-in-out infinite;
  }
}
</style>
