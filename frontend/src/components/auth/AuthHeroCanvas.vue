<!--
WebGL brand backdrop for the auth hero panel.

Renders the Nosdesk "N" glyph as a frosted-glass occluder lit by an
off-screen source: volumetric god rays, a spectral refraction through the
glyph, corona bloom, drifting fog and dust. Ported from the marketing-site
liquid-glass shader; the warm-gold palette is replaced with uniforms driven
by `--color-accent`, so workspace branding (and theme changes) carry through.

The glyph is a perspective-projected 3D slab (view rays intersected with
its front/back planes), tilted in space and swaying with the cursor. The
light is fixed off-screen; the cursor rotates the ray cone and the slab,
and an autonomous drift keeps both alive while idle. Honours
`prefers-reduced-motion` (single static frame). A perf governor shortens
the ray marches on a struggling GPU before it lowers the render scale (never
below 0.75), throttles to 30fps while the pointer is away, and falls back to
a CSS accent glow when WebGL2 is
unavailable or the GPU still can't keep up.
-->
<script setup lang="ts">
import { onMounted, onBeforeUnmount, ref, useTemplateRef } from 'vue';

const props = withDefaults(
  defineProps<{
    /** When false, the N mask is left blank so the lit backdrop (beams,
     *  fog, glow) renders without the brand glyph or its refraction. */
    showLogo?: boolean;
  }>(),
  { showLogo: true },
);

const parentRef = useTemplateRef<HTMLDivElement>('parent');
const canvasRef = useTemplateRef<HTMLCanvasElement>('canvas');
const fallback = ref(false);

// Nosdesk "N" mark (public/favicon.svg), square 78.4 viewBox.
const LOGO_PATH =
  'm 52.250001,0 h 22.06 v 78.4 l -20.58,-0.34 -27.58,-36.29 V 78.39 H 4.0900008 V 0 l 20.6400002,0.06 27.52,38.69 z';
const LOGO_VB = 78.4;
const LOGO_TEX_SCALE = 2;

const VERT = `#version 300 es
in vec2 a_pos;
out vec2 v_uv;
void main(){
  v_uv = a_pos * 0.5 + 0.5;
  gl_Position = vec4(a_pos, 0.0, 1.0);
}`;

// Single-pass volumetric light scattering (god rays) + glass refraction + bloom.
const FRAG = `#version 300 es
precision highp float;
in vec2 v_uv;
out vec4 fragColor;

uniform float u_time;      // 0->1 reveal progress, then stays at 1
uniform float u_anim;      // continuous seconds - drives ambient motion
uniform float u_breath;    // slow breathing oscillation
uniform vec2 u_light;      // light source position in UV space
uniform vec2 u_mouse;      // mouse position (UV, spring-smoothed)
uniform vec2 u_tilt;       // glyph slab orientation (pitch, yaw) radians
uniform float u_aspect;    // h/w
uniform vec2 u_resolution; // canvas pixel dimensions
uniform vec2 u_cssTexel;   // one CSS pixel in UV, so kernels keep their
                           // on-screen width whatever the render scale
uniform float u_qRays;     // 1.0 full; shortens the god-ray march (every fragment)
uniform float u_qGlass;    // 1.0 full; shortens the chromatic march (glyph only)
uniform float u_pxDensity; // backing pixels per CSS pixel (dpr x render scale)
uniform sampler2D u_logo;  // logo mask texture (white = logo)
uniform vec3 u_dark;       // accumulation base (near-black in both themes)
uniform vec3 u_warm;       // primary accent light
uniform vec3 u_hot;        // accent-tinted highlight
uniform vec3 u_corona;     // edge corona tint
uniform float u_lightMode; // 1.0 in light themes: re-composite at the end
uniform vec3 u_lightBase;  // panel base used in light themes

const int NUM_SAMPLES = 80;
const float DENSITY = 0.75;
const float DECAY = 0.96;
const float EXPOSURE = 0.06;

// Light-theme gain for the atmospheric ray glow (ambient wash, god rays,
// beams, lit fog, dust, flare, corona, edge rim). 0.0 = off, 1.0 = full.
// Kept off so the light panel reads clean and only the glyph treatment
// carries the accent; dark mode is unaffected (gated by u_lightMode).
const float RAY_GAIN_LIGHT = 0.0;

// Integer hash (PCG-style). fract(sin()) bands on Intel integrated GPUs
// even at highp; this stays uniform there and costs the same.
uint pcg(uint v){
  uint state = v * 747796405u + 2891336453u;
  uint word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
  return (word >> 22u) ^ word;
}
float hash(vec2 p){
  uvec2 q = uvec2(ivec2(floor(p * 64.0)));
  return float(pcg(q.x ^ pcg(q.y))) / 4294967295.0;
}
vec2 hash2(vec2 p){
  uvec2 q = uvec2(ivec2(floor(p * 64.0)));
  uint a = pcg(q.x ^ pcg(q.y));
  return vec2(float(a), float(pcg(a))) / 4294967295.0;
}

// Henyey-Greenstein phase function - directional scattering
float HGPhase(float cosTheta, float g){
  float gg = g * g;
  return (1.0 - gg) / (4.0 * 3.14159 * pow(1.0 + gg - 2.0 * g * cosTheta, 1.5));
}

// Beer's Law - exponential light absorption through medium
float beersLaw(float density, float dist){
  return exp(-density * dist);
}

void main(){
  vec2 uv = v_uv;
  float t = u_time; // 0 = dark, 1 = fully revealed

  vec2 ar = vec2(uv.x, uv.y * u_aspect);
  vec2 lightAR = vec2(u_light.x, u_light.y * u_aspect);

  // -- Perspective projection of the glyph slab --
  // The mark sits on a tilted plane viewed through a pinhole camera. Each
  // fragment's view ray is intersected with the slab's front and back
  // planes, giving perspective-correct texture coords (pUV / bUV); their
  // per-pixel difference drives the wall march below. u_tilt sways with
  // the smoothed cursor, so the slab rotates in 3D as the pointer moves.
  float sYaw = sin(u_tilt.y), cYaw = cos(u_tilt.y);
  float sPit = sin(u_tilt.x), cPit = cos(u_tilt.x);
  vec3 axU = vec3(cYaw, 0.0, -sYaw);                // plane x basis
  vec3 axV = vec3(sYaw * sPit, cPit, cYaw * sPit);  // plane y basis
  vec3 axN = vec3(sYaw * cPit, -sPit, cYaw * cPit); // plane normal
  const float FOCAL = 1.4; // camera distance - lower = wider perspective
  const float THICK = 0.08; // slab thickness in plane units
  vec3 cam = vec3(0.0, 0.0, FOCAL);
  vec3 rd = vec3(uv.x - 0.5, (uv.y - 0.5) * u_aspect, 0.0) - cam;
  float denom = dot(rd, axN);
  vec3 hF = cam + rd * (-dot(cam, axN) / denom);
  vec2 pUV = vec2(dot(hF, axU) + 0.5, dot(hF, axV) / u_aspect + 0.5);
  vec3 hB = cam + rd * (-(dot(cam, axN) + THICK) / denom);
  vec2 bUV = vec2(dot(hB, axU) + 0.5, dot(hB, axV) / u_aspect + 0.5);

  // -- Logo occluder --
  // Mask level of detail from the screen-space footprint, in uniform flow:
  // once the mask carries mips, implicit-LOD sampling inside the branches
  // below would be undefined. The marches add a bias for their step.
  vec2 maskSize = vec2(textureSize(u_logo, 0));
  vec2 lodDx = dFdx(pUV) * maskSize;
  vec2 lodDy = dFdy(pUV) * maskSize;
  float lod = max(0.0, 0.5 * log2(max(dot(lodDx, lodDx), dot(lodDy, lodDy))));
  float logo = textureLod(u_logo, pUV, lod).r;
  float occluder = 1.0 - logo;

  // -- Distributed ambient - biased toward where light originates --
  vec2 ambientCenter = vec2(0.7, 0.7 * u_aspect);
  float distToLogoCenter = length(ar - ambientCenter);
  float ambientWash = exp(-distToLogoCenter * 0.8) * 0.2;

  // -- Volumetric light scattering (god rays) --
  // Marched in plane space so occlusion follows the tilted slab.
  vec2 rayDir2d = normalize(pUV - u_light);
  float mouseAngle = (u_mouse.x - 0.5) * 1.6 + (u_mouse.y - 0.5) * 0.8;
  float baseAngle = 2.2; // ~126deg - up-left from light source
  float coneDir = baseAngle + mouseAngle;
  vec2 mainLightDir = vec2(cos(coneDir), sin(coneDir));
  float coneAngle = dot(rayDir2d, mainLightDir);
  float coneMask = smoothstep(-0.5, 0.3, coneAngle);

  int activeSamples = max(int(float(NUM_SAMPLES) * u_qRays), 24);
  vec2 deltaUV = rayDir2d * DENSITY / float(activeSamples);
  // Longer steps read a coarser mip, so fewer samples look prefiltered
  // rather than noisy.
  float rayLod = max(lod, clamp(log2(length(deltaUV * maskSize)), 0.0, 3.0));

  float dither = fract(hash(gl_FragCoord.xy) + fract(u_breath * 7.3));
  vec2 samplePos = pUV - deltaUV * dither;

  float cosTheta = dot(rayDir2d, normalize(u_light - vec2(0.5)));
  float phase = HGPhase(cosTheta, 0.6);

  float transmittance = 1.0;
  float godRays = 0.0;
  float hitLogo = 0.0;
  const float FOG_DENSITY = 0.35;
  // Quality shortens the march; the step grows to cover the same span, so
  // the integral (and the brightness) holds and only the smoothness drops.
  float STEP_LEN = DENSITY / float(activeSamples);

  for(int i = 0; i < NUM_SAMPLES; i++){
    if (i >= activeSamples) break;
    samplePos -= deltaUV;
    float logoSample = textureLod(u_logo, samplePos, rayLod).r;
    float occ = 1.0 - logoSample;
    hitLogo = max(hitLogo, logoSample);
    float scatterMask = hitLogo;
    float distToSrc = length(samplePos - u_light);
    float attenuation = exp(-0.3 * distToSrc);
    float stepDensity = FOG_DENSITY * occ;
    float stepTransmittance = beersLaw(stepDensity, STEP_LEN);
    godRays += occ * attenuation * phase * transmittance * STEP_LEN * scatterMask;
    transmittance *= stepTransmittance;
  }
  godRays *= EXPOSURE * 16.0 * coneMask;
  godRays *= t;

  // -- Corona glow around the logo edges --
  vec2 texel = 1.5 * u_cssTexel;
  float e1 = textureLod(u_logo, pUV + vec2(texel.x, 0.0), lod).r;
  float e2 = textureLod(u_logo, pUV - vec2(texel.x, 0.0), lod).r;
  float e3 = textureLod(u_logo, pUV + vec2(0.0, texel.y), lod).r;
  float e4 = textureLod(u_logo, pUV - vec2(0.0, texel.y), lod).r;
  float edge = length(vec2(e1 - e2, e3 - e4));

  vec2 toLight = normalize(u_light - pUV);
  vec2 edgeNormal = normalize(vec2(e1 - e2, e3 - e4));
  float facing = max(0.0, dot(edgeNormal, toLight));
  float corona = edge * facing * 2.0 * t;
  float coronaBloom = edge * 0.5 * t;

  // -- Palette (accent-driven) --
  vec3 darkBg = u_dark;
  vec3 warmGold = u_warm;
  vec3 hotWhite = u_hot;
  vec3 coronaColor = u_corona;

  float spatialFade = smoothstep(0.1, 0.8, uv.x) * smoothstep(0.0, 0.4, 1.0 - uv.y);

  // -- Volumetric light shafts - radiate from bottom-right --
  vec2 logoCenter = vec2(0.58, 0.45);
  // Origin well past the corner so the shafts arrive near-parallel
  // instead of visibly bursting from a point inside the panel.
  vec2 beamOrigin = vec2(1.18, 1.22) + (u_mouse - 0.5) * 0.08;
  vec2 beamDir = uv - beamOrigin;
  float beamAngle = atan(beamDir.y, beamDir.x);
  float beamDist = length(beamDir);

  float mouseRot = (u_mouse.x - 0.5) * 1.3 + (u_mouse.y - 0.5) * 0.7;
  float ba = beamAngle + mouseRot;

  float beam1 = pow(max(sin(ba * 3.0 + 0.5) * 0.5 + 0.5, 0.0), 3.0);
  float beam2 = pow(max(sin(ba * 2.0 + 1.8) * 0.5 + 0.5, 0.0), 3.0);
  float beam3 = pow(max(sin(ba * 5.0 + u_anim * 0.06) * 0.5 + 0.5, 0.0), 5.0);
  float beam4 = pow(max(sin(ba * 8.0 - u_anim * 0.04) * 0.5 + 0.5, 0.0), 6.0);

  float beams = beam1 * 0.35 + beam2 * 0.3 + beam3 * 0.2 + beam4 * 0.15;
  // Slower falloff compensates for the farther origin.
  beams *= smoothstep(0.0, 0.15, beamDist) * exp(-beamDist * 0.42);
  beams *= spatialFade * t;

  // -- Volumetric fog layers --
  // Driven by continuous time, not the reveal progress, so the fog and
  // dust keep drifting after the intro completes.
  float time = u_anim;
  float fog1 = sin(uv.x * 2.0 + uv.y * 1.5 + time * 0.12) *
               sin(uv.x * 3.0 - uv.y * 2.0 + time * 0.08);
  fog1 = fog1 * 0.5 + 0.5;
  float fog2 = sin(uv.x * 5.0 + uv.y * 4.0 - time * 0.1) *
               sin(uv.x * 7.0 - uv.y * 3.0 + time * 0.06);
  fog2 = fog2 * 0.5 + 0.5;
  float fog3 = sin(uv.x * 12.0 - uv.y * 8.0 + time * 0.15) *
               sin(uv.x * 9.0 + uv.y * 11.0 - time * 0.07);
  fog3 = fog3 * 0.5 + 0.5;
  float fog = fog1 * 0.5 + fog2 * 0.3 + fog3 * 0.2;
  fog = smoothstep(0.25, 0.75, fog);
  float litFog = fog * (0.03 + beams * 0.15);
  litFog *= spatialFade * t;
  litFog *= (0.4 + exp(-beamDist * 1.5) * 0.6);

  // -- Floating dust particles --
  float dust = 0.0;
  for(int i = 0; i < 3; i++){
    float fi = float(i);
    vec2 dustUV = uv * (20.0 + fi * 15.0);
    dustUV += vec2(time * (0.02 + fi * 0.01), time * (0.015 - fi * 0.005));
    vec2 dustCell = floor(dustUV);
    vec2 dustF = fract(dustUV) - 0.5;
    float dustRand = hash(dustCell + vec2(fi * 100.0));
    if(dustRand > 0.92){
      vec2 dustOff = hash2(dustCell + vec2(fi * 100.0)) * 0.4 - 0.2;
      float dustD = length(dustF - dustOff);
      float mote = (1.0 - smoothstep(0.0, 0.04 + fi * 0.01, dustD));
      mote *= (0.3 + beams * 2.0);
      mote *= sin(time * (2.0 + dustRand * 3.0) + dustRand * 6.28) * 0.3 + 0.7;
      dust += mote * 0.06;
    }
  }
  dust *= spatialFade * t;

  // -- Soft lens flare near logo center --
  float flareDist = length(pUV - logoCenter);
  float flare = exp(-flareDist * 5.0) * beams * 0.15;

  // -- Base scene --
  vec3 col = darkBg;
  col += warmGold * ambientWash * spatialFade;
  col += hotWhite * ambientWash * 0.3 * spatialFade;
  col += warmGold * godRays * 0.5 * (0.3 + spatialFade * 0.7);
  col += hotWhite * godRays * 0.2 * spatialFade;
  col += warmGold * beams * 0.3 * coneMask;
  col += hotWhite * beams * 0.18 * coneMask;
  col += warmGold * litFog * 0.5;
  col += warmGold * litFog * 0.9 * coneMask;
  col += hotWhite * litFog * 0.5 * coneMask;
  col += hotWhite * dust * 1.2;
  col += warmGold * flare * coneMask;
  col += hotWhite * flare * 0.2 * coneMask;

  // Light theme: tone the panel-filling ray glow (everything added above:
  // ambient wash, god rays, beams, lit fog, dust, flare) right down so the
  // light panel stays clean. The glyph's own refraction/edge treatment is
  // computed after this point and is preserved, becoming the only accent
  // carrier in light mode. Dark mode is unaffected (u_lightMode == 0).
  col = mix(col, darkBg, u_lightMode * (1.0 - RAY_GAIN_LIGHT));

  // -- Crystal slab walls --
  // March the view ray from the slab's front face to its back face; the
  // first covered step is a side wall, its index the distance behind the
  // front face. Direction and length come straight from the projection,
  // so the walls are perspective-correct and follow the slab's tilt.
  if (logo < 0.99) {
    const int DEPTH_STEPS = 14;
    vec2 depthOff = (bUV - pUV) / float(DEPTH_STEPS);

    float side = 0.0;
    float frontness = 0.0; // 1 at the front edge, ~0 at the back
    for (int i = 1; i <= DEPTH_STEPS; i++) {
      float cov = smoothstep(0.3, 0.7, textureLod(u_logo, pUV + depthOff * float(i), lod).r);
      float gain = max(cov - side, 0.0);
      frontness += gain * (1.0 - (float(i) - 1.0) / float(DEPTH_STEPS));
      side = max(side, cov);
    }

    if (side > 0.001) {
      float wall = side * (1.0 - logo) * t;
      float lip = frontness * frontness;
      // Tinted-glass transmission - the scene dims slightly through the
      // wall instead of being occluded by it.
      col *= 1.0 - wall * 0.3;
      // Per-channel falloff down the wall fakes dispersion: a white-hot
      // lip at the front face shifting through accent toward the back.
      vec3 spectral = vec3(pow(frontness, 1.2), pow(frontness, 1.8), pow(frontness, 2.6));
      col += mix(warmGold, hotWhite, lip) * spectral * wall * 0.7 * (0.45 + 0.55 * coneMask);
      // Faint accent body glow so the depth never reads as black.
      col += warmGold * wall * 0.08;
    }
  }

  // -- Frosted glass refraction inside the logo --
  if(logo > 0.01){
    vec2 fineTexel = max(0.5 * u_cssTexel, 1.0 / u_resolution);
    float f1 = textureLod(u_logo, pUV + vec2(fineTexel.x, 0.0), lod).r;
    float f2 = textureLod(u_logo, pUV - vec2(fineTexel.x, 0.0), lod).r;
    float f3 = textureLod(u_logo, pUV + vec2(0.0, fineTexel.y), lod).r;
    float f4 = textureLod(u_logo, pUV - vec2(0.0, fineTexel.y), lod).r;
    vec2 fineNormal = vec2(f1 - f2, f3 - f4);

    vec2 medTexel = 1.5 * u_cssTexel;
    float m1 = textureLod(u_logo, pUV + vec2(medTexel.x, 0.0), lod).r;
    float m2 = textureLod(u_logo, pUV - vec2(medTexel.x, 0.0), lod).r;
    float m3 = textureLod(u_logo, pUV + vec2(0.0, medTexel.y), lod).r;
    float m4 = textureLod(u_logo, pUV - vec2(0.0, medTexel.y), lod).r;
    vec2 medNormal = vec2(m1 - m2, m3 - m4);

    // Broad third scale - the chamfer width for the 3D face shading.
    vec2 bevTexel = 4.5 * u_cssTexel;
    float b1 = textureLod(u_logo, pUV + vec2(bevTexel.x, 0.0), lod).r;
    float b2 = textureLod(u_logo, pUV - vec2(bevTexel.x, 0.0), lod).r;
    float b3 = textureLod(u_logo, pUV + vec2(0.0, bevTexel.y), lod).r;
    float b4 = textureLod(u_logo, pUV - vec2(0.0, bevTexel.y), lod).r;
    vec2 bevNormal = vec2(b1 - b2, b3 - b4);

    vec2 normal2d = normalize(fineNormal * 0.6 + medNormal * 0.4 + 0.001);
    float normalStrength = length(fineNormal * 0.6 + medNormal * 0.4);
    float edgeFactor = smoothstep(0.0, 0.15, normalStrength);

    vec2 viewDir = normalize(pUV - vec2(0.5));
    float fresnel = pow(1.0 - abs(dot(viewDir, normal2d)), 2.5) * 0.6;

    vec2 frost = (hash2(gl_FragCoord.xy + fract(u_breath * 3.7)) - 0.5) * 0.006
      * mix(0.5, 1.0, clamp((u_pxDensity - 1.0) / 0.5, 0.0, 1.0));

    float iorR = 0.03, iorG = 0.10, iorB = 0.20;
    float caustic = sin(dot(pUV, vec2(40.0, 30.0)) + u_anim * 0.8) *
                    sin(dot(pUV, vec2(25.0, -45.0)) - u_anim * 0.6) * 0.3;
    iorR *= (1.0 + caustic * 0.4);
    iorG *= (1.0 + caustic * 0.2);
    iorB *= (1.0 - caustic * 0.3);

    vec2 lightToSurface = normalize(pUV - u_light);
    vec2 refDir = normalize(-normal2d + lightToSurface * 0.5);

    vec2 refR = (refDir * iorR + frost) * logo;
    vec2 refG = (refDir * iorG + frost * 0.7) * logo;
    vec2 refB = (refDir * iorB + frost * 0.4) * logo;

    const int CH_SAMPLES = 60;
    const float CH_DENSITY = 0.75;
    const float CH_DECAY = 0.97;
    // Fewer, longer steps at lower quality; the per-step decay is raised
    // to the same power so the falloff over the span is unchanged, and the
    // sum is rescaled to the full-quality sample count.
    int activeCh = max(int(float(CH_SAMPLES) * u_qGlass), 30);
    float chStepScale = float(CH_SAMPLES) / float(activeCh);
    float chDecay = pow(CH_DECAY, chStepScale);

    vec2 uvR = pUV + refR;
    vec2 uvG = pUV + refG;
    vec2 uvB = pUV + refB;

    vec2 dirR = normalize(uvR - u_light) * CH_DENSITY / float(activeCh);
    vec2 dirG = normalize(uvG - u_light) * CH_DENSITY / float(activeCh);
    vec2 dirB = normalize(uvB - u_light) * CH_DENSITY / float(activeCh);

    float chLod = max(lod, clamp(log2(length(dirG * maskSize)), 0.0, 3.0));
    float raysR = 0.0, raysG = 0.0, raysB = 0.0;
    float decR = 1.0, decG = 1.0, decB = 1.0;
    vec2 posR = uvR - dirR * dither;
    vec2 posG = uvG - dirG * dither;
    vec2 posB = uvB - dirB * dither;

    for(int j = 0; j < CH_SAMPLES; j++){
      if (j >= activeCh) break;
      posR -= dirR;
      posG -= dirG;
      posB -= dirB;
      raysR += (1.0 - textureLod(u_logo, posR, chLod).r) * decR;
      raysG += (1.0 - textureLod(u_logo, posG, chLod).r) * decG;
      raysB += (1.0 - textureLod(u_logo, posB, chLod).r) * decB;
      decR *= chDecay;
      decG *= chDecay;
      decB *= chDecay;
    }
    raysR *= chStepScale;
    raysG *= chStepScale;
    raysB *= chStepScale;

    float cosR = dot(normalize(uvR - u_light), normalize(u_light - vec2(0.5)));
    float cosG = dot(normalize(uvG - u_light), normalize(u_light - vec2(0.5)));
    float cosB = dot(normalize(uvB - u_light), normalize(u_light - vec2(0.5)));
    float phaseR = HGPhase(cosR, 0.25);
    float phaseG = HGPhase(cosG, 0.5);
    float phaseB = HGPhase(cosB, 0.75);
    float phaseMax = max(phaseR, max(phaseG, phaseB)) + 0.001;

    vec3 refractedLight = vec3(
      raysR * (phaseR / phaseMax),
      raysG * (phaseG / phaseMax),
      raysB * (phaseB / phaseMax)
    ) * EXPOSURE * 3.0 * t;

    vec2 glassRayDir2d = normalize(pUV - u_light);
    float glassConeAngle = dot(glassRayDir2d, mainLightDir);
    float glassCone = smoothstep(-0.4, 0.3, glassConeAngle);
    refractedLight *= glassCone;

    refractedLight = refractedLight / (refractedLight + 2.5);

    vec2 toLightDir = normalize(u_light - pUV);
    vec3 bloom = vec3(0.0);
    for(int b = 0; b < 6; b++){
      float angle = float(b) * 1.047;
      vec2 dir = vec2(cos(angle), sin(angle));
      float lightAlign = max(0.0, dot(dir, toLightDir));
      vec2 off1 = dir * 2.0 * u_cssTexel;
      float bE1 = abs(textureLod(u_logo, pUV + off1, lod).r - textureLod(u_logo, pUV - off1, lod).r);
      vec2 off2 = dir * 5.0 * u_cssTexel;
      float bE2 = abs(textureLod(u_logo, pUV + off2, lod).r - textureLod(u_logo, pUV - off2, lod).r);
      bloom += refractedLight * (bE1 * 0.6 + bE2 * 0.4) * lightAlign;
    }
    bloom /= 3.0;

    float edgeLightFacing = max(0.0, dot(normal2d, toLightDir));
    float litEdge = edgeFactor * (0.3 + edgeLightFacing * 0.7) * glassCone;

    vec3 glassCol = col * 0.7;
    glassCol += refractedLight * (0.5 + litEdge * 1.2) * glassCone;
    glassCol += bloom * 2.5;

    float causticLight = pow(max(caustic, 0.0), 2.0) * 0.05 * litEdge;
    glassCol += vec3(1.0, 0.9, 0.7) * causticLight * t;

    float specular = pow(max(dot(reflect(-normalize(pUV - u_light), normal2d), viewDir), 0.0), 8.0);
    glassCol += warmGold * specular * 0.15 * t * litEdge;

    glassCol += warmGold * fresnel * t * 0.18 * litEdge;

    // -- Chamfer specular --
    // Height-field normal (negated gradient, z up) turns the rim into a
    // chamfer; light-facing chamfers catch a crisp Blinn-Phong highlight.
    // No diffuse multiply - the refraction owns the face brightness.
    vec2 bevGrad = fineNormal * 0.5 + medNormal * 0.8 + bevNormal * 1.6;
    vec3 N3 = normalize(vec3(-bevGrad * 2.0, 1.0));
    vec3 L3 = normalize(vec3(u_light - pUV, 0.4));
    vec3 H3 = normalize(L3 + vec3(0.0, 0.0, 1.0));
    glassCol += hotWhite * pow(max(dot(N3, H3), 0.0), 28.0) * 0.35 * t * glassCone;

    col = mix(col, glassCol, logo);
  }

  // Snapshot before the ambient corona/edge glow so light mode can drop
  // those (they're panel glow, not glyph treatment) while keeping the
  // glyph composited above.
  vec3 preGlow = col;

  // -- Corona - ambient base + directional boost --
  col += warmGold * corona * u_breath * 0.15;
  col += warmGold * corona * u_breath * 0.2 * coneMask;
  col += coronaColor * coronaBloom * u_breath * 0.1 * coneMask;

  // -- Edge rim light --
  col += warmGold * edge * t * 0.03 * u_breath;
  col += warmGold * edge * t * 0.03 * u_breath * coneMask;

  // Light theme: revert the corona/edge ambient glow (keep the glyph).
  col = mix(col, preGlow, u_lightMode * (1.0 - RAY_GAIN_LIGHT));

  // -- Light-theme re-composite --
  // Everything above paints accent light additively onto the near-black
  // base, which only reads on a dark panel. In light themes, take that
  // accumulated energy (how far col rose above the base) and instead paint
  // it as accent ink on a light panel, so the same animated beams, corona,
  // and refraction read as soft accent streaks rather than a glow on black.
  if (u_lightMode > 0.5) {
    float energy = clamp(dot(col - darkBg, vec3(0.299, 0.587, 0.114)) * 1.4, 0.0, 1.0);
    vec3 ink = mix(u_lightBase, u_warm, 0.85);
    col = mix(u_lightBase, ink, energy);
  }

  // Animated +-1 LSB grain dissolves 8-bit banding in the dark falloffs.
  col += (hash(gl_FragCoord.xy + fract(u_anim) * 100.0) - 0.5) / 255.0 * 2.0;

  fragColor = vec4(col, 1.0);
}`;

type Rgb = [number, number, number];

function hexToRgb01(hex: string): Rgb | null {
  const m = hex.trim().replace('#', '');
  const full = m.length === 3 ? m.split('').map((c) => c + c).join('') : m;
  if (full.length !== 6 || /[^0-9a-fA-F]/.test(full)) return null;
  return [
    parseInt(full.slice(0, 2), 16) / 255,
    parseInt(full.slice(2, 4), 16) / 255,
    parseInt(full.slice(4, 6), 16) / 255,
  ];
}

const mix = (a: Rgb, b: Rgb, t: number): Rgb => [
  a[0] + (b[0] - a[0]) * t,
  a[1] + (b[1] - a[1]) * t,
  a[2] + (b[2] - a[2]) * t,
];

// Derive the shader palette from the live theme tokens. The accent drives
// the lit colours (so workspace branding carries through); the page-bg
// token's luminance tells us whether we're in a light theme, in which case
// the shader re-composites onto `lightBase` (see the fragment shader).
function palette(): {
  dark: Rgb;
  warm: Rgb;
  hot: Rgb;
  corona: Rgb;
  lightMode: boolean;
  lightBase: Rgb;
} {
  const styles = getComputedStyle(document.documentElement);
  const accent = hexToRgb01(styles.getPropertyValue('--color-accent')) ?? [1.0, 0.42, 0.1];
  const white: Rgb = [1, 1, 1];
  // Theme polarity from the app background token: light themes have a light
  // page background, dark themes a dark one.
  const appBg = hexToRgb01(styles.getPropertyValue('--color-app')) ?? [0.04, 0.04, 0.04];
  const lum = appBg[0] * 0.299 + appBg[1] * 0.587 + appBg[2] * 0.114;
  return {
    // Accumulation base stays near-black in both themes; light mode discards
    // it in the re-composite and uses lightBase instead.
    dark: [0.031, 0.035, 0.039],
    warm: accent,
    hot: mix(accent, white, 0.72),
    corona: mix(accent, white, 0.25),
    lightMode: lum > 0.5,
    lightBase: [0.961, 0.965, 0.973], // #f5f6f8 - shared with AuthLayout
  };
}

// Texel cap for the mask. The panel's CSS size explodes at far-out browser
// zoom (5000+ CSS px at 25%) while its physical size stays the same; an
// uncapped allocation there means a ~10k-px canvas and a huge texture
// upload. 2048 texels across is indistinguishable on screen.
const LOGO_TEX_MAX = 2048;

// Rasterise the "N" glyph to a mask canvas, centred on the panel.
function createLogoTexture(w: number, h: number, drawLogo: boolean): HTMLCanvasElement {
  const texScale = Math.min(LOGO_TEX_SCALE, LOGO_TEX_MAX / Math.max(w, h, 1));
  const c = document.createElement('canvas');
  c.width = Math.max(1, Math.round(w * texScale));
  c.height = Math.max(1, Math.round(h * texScale));
  const ctx = c.getContext('2d')!;
  ctx.scale(texScale, texScale);
  const size = Math.min(w, h) * 0.42;
  const scale = size / LOGO_VB;
  const drawn = LOGO_VB * scale;
  ctx.translate(w * 0.52 - drawn / 2, h * 0.48 - drawn / 2);
  ctx.scale(scale, scale);
  ctx.fillStyle = 'white';
  // Skip the glyph to keep the lit backdrop without the N (blank mask =
  // all the logo-derived god rays / corona / refraction fall to zero).
  if (drawLogo) ctx.fill(new Path2D(LOGO_PATH));
  return c;
}

function initWebGL(canvas: HTMLCanvasElement, logoCanvas: HTMLCanvasElement) {
  // alpha:false - the shader always writes opaque, so an opaque canvas
  // skips the compositor's blend pass.
  const gl = canvas.getContext('webgl2', {
    alpha: false,
    antialias: false,
  });
  if (!gl) return null;
  if ('drawingBufferColorSpace' in gl) {
    try {
      (gl as unknown as { drawingBufferColorSpace: string }).drawingBufferColorSpace = 'display-p3';
    } catch {
      /* unsupported, stays sRGB */
    }
  }

  const compile = (type: number, src: string) => {
    const sh = gl.createShader(type)!;
    gl.shaderSource(sh, src);
    gl.compileShader(sh);
    if (!gl.getShaderParameter(sh, gl.COMPILE_STATUS)) {
      console.error(gl.getShaderInfoLog(sh));
      return null;
    }
    return sh;
  };

  const vs = compile(gl.VERTEX_SHADER, VERT);
  const fs = compile(gl.FRAGMENT_SHADER, FRAG);
  if (!vs || !fs) return null;

  const prog = gl.createProgram()!;
  gl.attachShader(prog, vs);
  gl.attachShader(prog, fs);
  gl.linkProgram(prog);
  if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) {
    console.error(gl.getProgramInfoLog(prog));
    return null;
  }
  gl.useProgram(prog);

  const buf = gl.createBuffer()!;
  gl.bindBuffer(gl.ARRAY_BUFFER, buf);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);
  const aPos = gl.getAttribLocation(prog, 'a_pos');
  gl.enableVertexAttribArray(aPos);
  gl.vertexAttribPointer(aPos, 2, gl.FLOAT, false, 0, 0);

  const tex = gl.createTexture()!;
  gl.activeTexture(gl.TEXTURE0);
  gl.bindTexture(gl.TEXTURE_2D, tex);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
  // Mipmapped: the mask is 2x CSS while the frame is CSS x dpr x scale, so
  // it is minified on every 1x panel and diagonals undersample without a
  // chain. The shader samples with textureLod (see `lod` there).
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR_MIPMAP_LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
  gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, true);
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, logoCanvas);
  gl.generateMipmap(gl.TEXTURE_2D);

  const loc = (n: string) => gl.getUniformLocation(prog, n)!;
  gl.uniform1i(loc('u_logo'), 0);

  return {
    gl,
    tex,
    u: {
      time: loc('u_time'),
      anim: loc('u_anim'),
      breath: loc('u_breath'),
      light: loc('u_light'),
      mouse: loc('u_mouse'),
      tilt: loc('u_tilt'),
      aspect: loc('u_aspect'),
      resolution: loc('u_resolution'),
      cssTexel: loc('u_cssTexel'),
      qRays: loc('u_qRays'),
      qGlass: loc('u_qGlass'),
      pxDensity: loc('u_pxDensity'),
      dark: loc('u_dark'),
      warm: loc('u_warm'),
      hot: loc('u_hot'),
      corona: loc('u_corona'),
      lightMode: loc('u_lightMode'),
      lightBase: loc('u_lightBase'),
    },
  };
}

let cleanup: (() => void) | null = null;

onMounted(() => {
  const canvas = canvasRef.value;
  const parent = parentRef.value;
  if (!canvas || !parent) return;

  const isStatic = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  const noCursor = isStatic ||
    window.matchMedia('(pointer: coarse)').matches ||
    window.innerWidth < 1024;

  const w0 = parent.offsetWidth || 1200;
  const h0 = parent.offsetHeight || 800;
  const webgl = initWebGL(canvas, createLogoTexture(w0, h0, props.showLogo));
  if (!webgl) {
    fallback.value = true;
    return;
  }

  const applyColors = () => {
    const { gl, u } = webgl;
    const p = palette();
    gl.uniform3fv(u.dark, p.dark);
    gl.uniform3fv(u.warm, p.warm);
    gl.uniform3fv(u.hot, p.hot);
    gl.uniform3fv(u.corona, p.corona);
    gl.uniform1f(u.lightMode, p.lightMode ? 1 : 0);
    gl.uniform3fv(u.lightBase, p.lightBase);
  };
  applyColors();

  // Re-tint when the theme or branding accent changes at runtime.
  const themeObserver = new MutationObserver(applyColors);
  themeObserver.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ['data-theme', 'style', 'class'],
  });

  let visible = true;
  let raf = 0;
  let mounted = true;
  let mouseX = 0.75;
  let mouseY = 0.25;
  let smoothX = 0.75;
  let smoothY = 0.25;
  let elapsed = 0;
  let lastFrame = -1;
  let lastPointerMove = 0;
  let hasPointer = false;
  // Idle orbit (see draw): a slow 2:3 Lissajous around the pose the remaps
  // were tuned for, entered from wherever the pointer stopped.
  const ORBIT_CENTRE = { x: 0.75, y: 0.25 };
  const ORBIT_RADIUS = 0.15;
  const ORBIT_PERIOD_X = 180_000; // ms per revolution on x
  const ORBIT_PERIOD_Y = 270_000;
  const ORBIT_BLEND_MS = 25_000;
  const driftPhase = (w0 % 97) / 13; // deterministic but varied start
  let idleSince = -1; // animation time (ms) the idle began, -1 while active
  let idleHold = { x: 0.75, y: 0.25 }; // where the pointer stopped
  let idlePhase0 = 0; // orbit phase nearest the hold, so entry is seamless

  const INTRO_MS = 5000;
  const IDLE_TIMEOUT = 3000;
  const SMOOTH_FACTOR = 0.03;

  // Perf governor - FPS measured in 60-frame windows. A struggling GPU
  // first drops the internal render scale (invisible on content this
  // soft); the CSS glow fallback is the last resort. Once a window holds
  // the target, monitoring stops.
  // Perf governor. The first window is thrown away (it includes shader
  // compile and page-load JS) and used as the display's frame rate, so a
  // 30 Hz panel is not mistaken for a struggling GPU. Then, per window:
  // below 75% of the display rate step one tier down, below a third fall
  // back; after three clean windows step one tier up. Tiers cut the two
  // ray marches before touching resolution, since resolution is what the
  // eye sees on the glyph edges; the scale never drops below 0.75, where
  // the per-pixel dither, frost and grain would turn into 2x2 blocks.
  const FPS_WINDOW = 60;
  const TIERS: { rays: number; glass: number; scale: number }[] = [
    { rays: 1.0, glass: 1.0, scale: 1 },
    { rays: 0.5, glass: 0.75, scale: 1 },
    { rays: 0.3, glass: 0.5, scale: 1 },
    { rays: 0.3, glass: 0.5, scale: 0.75 },
  ];
  let tier = 0;
  let displayFps = 0; // 0 until the first window has measured it
  let cleanWindows = 0;
  let frameCount = 0;
  let windowStart = 0;
  let governing = true;
  let renderScale = 1;
  let lastDraw = 0;
  // Hard ceiling on backing-store pixels: dpr already folds browser zoom
  // in (physical size is zoom-invariant), this bounds fragment load on
  // very large or high-density panels.
  const MAX_RENDER_PIXELS = 2600 * 1600;
  let logoDirty = 0; // timestamp of a pending mask rebuild, -1 = now

  const io = new IntersectionObserver(
    ([e]) => {
      visible = e.isIntersecting;
      if (!isStatic && visible && !raf) raf = requestAnimationFrame(loop);
    },
    { threshold: 0 },
  );
  io.observe(canvas);

  // Track across the whole viewport, not just the hero panel, so cursor
  // movement over the form column on the left also drives the tilt and ray
  // cone. Normalised to window size (CSS px), so the full page width maps
  // to the [0,1] sway range the draw() remaps expect.
  const onPointer = (e: PointerEvent) => {
    mouseX = e.clientX / window.innerWidth;
    mouseY = e.clientY / window.innerHeight;
    lastPointerMove = performance.now();
    hasPointer = true;
  };
  if (!noCursor) window.addEventListener('pointermove', onPointer, { capture: true });

  const draw = (t: number) => {
    const { gl, u, tex } = webgl;
    const w = parent.offsetWidth;
    const h = parent.offsetHeight;
    if (w < 2 || h < 2) return; // hidden (sub-lg) or mid-layout

    const now = performance.now();
    const isIdle = !hasPointer || now - lastPointerMove > IDLE_TIMEOUT;
    if (isIdle) {
      // Driven by animation time, not per-frame increments, so the 30fps
      // idle throttle does not halve the speed. The orbit starts at the
      // phase nearest where the pointer stopped and the target blends from
      // the hold onto the path over 25s, so entry never jumps. Bounded:
      // yaw stays within [-0.23, -0.13] rad and pitch within
      // [-0.01, 0.05] rad, never pointing down.
      if (idleSince < 0) {
        idleSince = t;
        idleHold = { x: smoothX, y: smoothY };
        idlePhase0 = Math.atan2(smoothY - ORBIT_CENTRE.y, smoothX - ORBIT_CENTRE.x);
      }
      const since = t - idleSince;
      const theta = idlePhase0 + (since / ORBIT_PERIOD_X) * Math.PI * 2;
      const orbit = {
        x: ORBIT_CENTRE.x + ORBIT_RADIUS * Math.cos(theta),
        y: ORBIT_CENTRE.y + ORBIT_RADIUS * Math.sin(theta * (ORBIT_PERIOD_X / ORBIT_PERIOD_Y) + driftPhase),
      };
      const blendT = Math.min(since / ORBIT_BLEND_MS, 1);
      const blend = blendT * blendT * (3 - 2 * blendT);
      const targetX = idleHold.x + (orbit.x - idleHold.x) * blend;
      const targetY = idleHold.y + (orbit.y - idleHold.y) * blend;
      smoothX += (targetX - smoothX) * 0.01;
      smoothY += (targetY - smoothY) * 0.01;
    } else {
      idleSince = -1;
      smoothX += (mouseX - smoothX) * SMOOTH_FACTOR;
      smoothY += (mouseY - smoothY) * SMOOTH_FACTOR;
    }

    const progress = Math.min(t / INTRO_MS, 1);
    const eased = 1 - Math.pow(1 - progress, 3);
    const breath = progress >= 1 ? Math.sin(t * 0.0008) * 0.08 + 1.0 : 1.0;

    let dpr = Math.min(window.devicePixelRatio || 1, 2) * renderScale;
    dpr = Math.min(dpr, Math.sqrt(MAX_RENDER_PIXELS / (w * h)));
    const pw = Math.floor(w * dpr);
    const ph = Math.floor(h * dpr);
    if (canvas.width !== pw || canvas.height !== ph) {
      canvas.width = pw;
      canvas.height = ph;
      gl.viewport(0, 0, pw, ph);
      // Defer the mask rebuild so continuous resize/zoom doesn't pay a
      // rasterise + texture upload every frame; the briefly stretched
      // mask is imperceptible. Static mode has no later frame: rebuild now.
      logoDirty = isStatic ? -1 : performance.now();
    }
    if (logoDirty !== 0 && (logoDirty < 0 || performance.now() - logoDirty > 150)) {
      logoDirty = 0;
      gl.activeTexture(gl.TEXTURE0);
      gl.bindTexture(gl.TEXTURE_2D, tex);
      gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, true);
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, createLogoTexture(w, h, props.showLogo));
      gl.generateMipmap(gl.TEXTURE_2D);
    }

    gl.uniform1f(u.time, eased);
    gl.uniform1f(u.anim, t * 0.001);
    gl.uniform1f(u.breath, breath);
    // Well past the top-right corner: the farther the source, the more
    // parallel the rays read on screen (no visible convergence point).
    gl.uniform2f(u.light, 1.5, 1.6);
    const remappedX = Math.max(0.1, 0.4 + smoothX * 0.5);
    const remappedY = 0.15 + smoothY * 0.45;
    gl.uniform2f(u.mouse, remappedX, 1.0 - remappedY);
    // Slab orientation: resting pose ~-12deg yaw / ~6deg pitch, swaying
    // with the smoothed cursor (and the idle drift through it).
    gl.uniform2f(u.tilt, 0.07 + (smoothY - 0.5) * 0.2, -0.26 + (smoothX - 0.5) * 0.32);
    gl.uniform1f(u.aspect, h / w);
    gl.uniform2f(u.resolution, pw, ph);
    gl.uniform2f(u.cssTexel, 1 / w, 1 / h);
    gl.uniform1f(u.qRays, TIERS[tier].rays);
    gl.uniform1f(u.qGlass, TIERS[tier].glass);
    gl.uniform1f(u.pxDensity, dpr);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
  };

  const loop = (time: number) => {
    raf = 0;
    if (!visible || !mounted) {
      lastFrame = -1;
      return;
    }
    if (lastFrame >= 0) elapsed += Math.min(time - lastFrame, 100);
    lastFrame = time;

    if (governing) {
      if (frameCount === 0) windowStart = time;
      frameCount++;
      if (frameCount === FPS_WINDOW) {
        const avgFps = (FPS_WINDOW / (time - windowStart)) * 1000;
        frameCount = 0;
        if (displayFps === 0) {
          // Warm-up window: take it as the display rate, capped to the
          // common panels so a slow first window never sets a low bar.
          displayFps = Math.min(Math.max(avgFps, 30), 120);
        } else if (avgFps < displayFps / 3) {
          if (tier === TIERS.length - 1) {
            fallback.value = true;
            return;
          }
          tier = TIERS.length - 1;
          cleanWindows = 0;
        } else if (avgFps < displayFps * 0.75) {
          if (tier < TIERS.length - 1) tier += 1;
          cleanWindows = 0;
        } else if (tier > 0) {
          cleanWindows += 1;
          if (cleanWindows >= 3) {
            tier -= 1;
            cleanWindows = 0;
          }
        } else {
          governing = false; // full quality holding the display rate
        }
        renderScale = TIERS[tier].scale;
      }
    } else if (elapsed > INTRO_MS) {
      // Idle throttle: with the pointer away this is a slow ambient glow,
      // so 30fps is indistinguishable and halves the GPU load. Animation
      // time keeps accumulating, only the draw is skipped.
      const idle = !hasPointer || performance.now() - lastPointerMove > IDLE_TIMEOUT;
      if (idle && time - lastDraw < 31) {
        raf = requestAnimationFrame(loop);
        return;
      }
    }

    lastDraw = time;
    draw(elapsed);
    raf = requestAnimationFrame(loop);
  };

  if (isStatic) {
    draw(INTRO_MS);
    const ro = new ResizeObserver(() => draw(INTRO_MS));
    ro.observe(parent);
    cleanup = () => {
      mounted = false;
      io.disconnect();
      ro.disconnect();
      themeObserver.disconnect();
    };
    return;
  }

  raf = requestAnimationFrame(loop);
  cleanup = () => {
    mounted = false;
    cancelAnimationFrame(raf);
    io.disconnect();
    themeObserver.disconnect();
    if (!noCursor) window.removeEventListener('pointermove', onPointer, { capture: true });
  };
});

onBeforeUnmount(() => {
  cleanup?.();
  cleanup = null;
});
</script>

<template>
  <div ref="parent" class="absolute inset-0">
    <canvas
      ref="canvas"
      class="absolute inset-0 h-full w-full"
      aria-hidden="true"
    ></canvas>
    <!-- CSS fallback: static accent glow when WebGL2 is unavailable. -->
    <div v-if="fallback" class="hero-fallback absolute inset-0" aria-hidden="true"></div>
  </div>
</template>

<style scoped>
.hero-fallback {
  background:
    radial-gradient(ellipse 600px 600px at 60% 45%,
      color-mix(in srgb, var(--color-accent) 28%, transparent), transparent 70%),
    radial-gradient(ellipse 900px 700px at 90% 90%,
      color-mix(in srgb, var(--color-accent) 14%, transparent), transparent 65%);
}
</style>
