<!-- ErrorView.vue -->
<script setup lang="ts">
import { useRoute, useRouter } from "vue-router";
import { onMounted, onUnmounted, ref, reactive, watchEffect, computed } from "vue";

// Type definition for the direction of a glitch spike.
// 'vertical' = vertical glitch, 'horizontal' = horizontal glitch, null = no glitch
type SpikeDirection = "vertical" | "horizontal" | null;

// Color channel identifiers
type ColorChannel = "red" | "green" | "blue";
const CHANNELS: ColorChannel[] = ["red", "green", "blue"];

// Vue Router instances for accessing route parameters and navigation.
const route = useRoute();
const router = useRouter();

// Navigation functions.
const goBack = () => router.back();
const goHome = () => router.push("/");

// --- Refs for SVG Elements ---
const svg = ref<SVGSVGElement | null>(null); // Ref for the main SVG element, used to get its dimensions for mouse normalization.

// Store channel refs in arrays for easier access
const turbulenceRefs = ref<SVGFETurbulenceElement[]>([]);
const offsetRefs = ref<SVGFEOffsetElement[]>([]);
const displacementRefs = ref<SVGFEDisplacementMapElement[]>([]);
const colorMatrixRefs = ref<SVGFEColorMatrixElement[]>([]);

// --- Animation State & Mouse Tracking Refs ---
const frameId = ref<number>(0); // Stores the ID of the requestAnimationFrame, used to cancel the animation.
const seedCounter = ref(0); // Used to animate the 'seed' for feTurbulence, making the noise pattern evolve over time.
// The animation uses direct cursor input (no smoothing/velocity calculations) for immediate visual response

// SVG responsive dimensions
const svgWidth = ref('60rem'); // Default SVG width
const svgHeight = ref('24rem'); // Default SVG height
const fontSize = ref('14rem'); // Default font size
const errorCode = ref('404'); // Default error code
const errorMessage = ref('Page not found'); // Default error message

// Raw mouse coordinates (relative to the viewport, not SVG).
const rawMouseX = ref(0);
const rawMouseY = ref(0);

// Normalized mouse positions (0-1 relative to SVG dimensions)
// Direct cursor input without smoothing for immediate response
const normMouseX = ref(0.5);
const normMouseY = ref(0.5);
const mouseHasMoved = ref(false); // Flag to track initial mouse movement
const isMouseOverSvg = ref(false); // Flag to track if mouse is over SVG

// --- Glitch State Variables (Unified into arrays) ---
// glitchCounters: Frame countdown for how long a glitch is active for each channel
const glitchCounters = ref<number[]>([0, 0, 0]);
// spikeDirections: Whether the current glitch is vertical or horizontal for each channel
const spikeDirections = ref<(SpikeDirection | null)[]>([null, null, null]);

// --- Click Interaction Variables ---
// Clicking creates a temporary distortion effect centered on cursor position
// This effect intensifies all glitch parameters temporarily and then decays
const clickGlitchIntensity = ref(0); // Current intensity of click-triggered effects (0-1)
const clickGlitchDecay = 0.75; // Rate at which click effect decays (smaller = faster decay)
const maxClickGlitchIntensity = 0.4; // Maximum intensity when clicked (gentler effect)
const clickLocationX = ref(0); // X position of the last click (normalized 0-1) - direct from cursor
const clickLocationY = ref(0); // Y position of the last click (normalized 0-1) - direct from cursor
const isClickable = ref(true); // Prevents rapid click spam by throttling clicks

// Phase offset settings for each channel (index 0=red, 1=green, 2=blue)
const channelPhaseOffsets = [
  { driftX: 0.3, driftY: 0.2, wobblePhase: 0, cursorXFactor: 0.7, cursorYFactor: 0.7 },
  { driftX: 0.25, driftY: 0.35, wobblePhase: 2, cursorXFactor: 0.8, cursorYFactor: 0.8 },
  { driftX: 0.4, driftY: 0.15, wobblePhase: 4, cursorXFactor: 1.2, cursorYFactor: 1.2 }
];

// Frequency settings for each channel
const channelFrequencySettings = [
  { baseX: 0.0020, baseY: 0.0020 }, // Red
  { baseX: 0.0022, baseY: 0.0022 }, // Green
  { baseX: 0.0018, baseY: 0.0018 }  // Blue
];

// Glitch frequency settings for each channel/direction
const glitchFrequencySettings = {
  horizontal: { baseX: 0.010, baseY: 0.0010, xAmpFactor: 0.015, yAmpFactor: 0.0015 },
  vertical: { baseX: 0.0010, baseY: 0.010, xAmpFactor: 0.0015, yAmpFactor: 0.015 }
};

// General time variable for continuous animation (used in sine/cosine oscillations).
let wobbleTime = 0.005;

// --- Simplified Effect Parameters ---
// Core parameters that drive the effect
let masterEffectEnabled = true; // Master toggle for all effects
let globalEffectIntensity = 0.5; // Overall strength of all effects
let baseGlitchIntensity = 0.25; // Glitch jitter amplitude
let cursorInfluence = 0.8; // Combined cursor influence parameter
let channelSeparation = 1.5; // Controls how far apart RGB channels drift
let distortionScale = 15; // Base displacement map scale

// --- Debug Controls ---
const showDebug = ref(false);

// New simplified parameter system
const debugControls = reactive({
  // Main Controls
  masterEffectEnabled,
  globalEffectIntensity, // Main control for overall effect strength
  
  // Essential Parameters
  channelSeparation: 1.0, // Controls how far apart the RGB channels are
  distortionScale: 1.0, // Controls the scale of the displacement map
  glitchFrequency: 0.15, // How often glitches occur (0-1)
  glitchIntensity: 1.0, // How strong the glitches are
  cursorInfluence: 0.1, // Combined parameter for all cursor interactions
  
  // Toggle for advanced parameters (hidden by default)
  showAdvanced: false
});

// Sync debugControls -> mutable params
watchEffect(() => {
  masterEffectEnabled = debugControls.masterEffectEnabled;
  globalEffectIntensity = debugControls.globalEffectIntensity;
  
  // Map simplified controls to core parameters
  baseGlitchIntensity = 0.25 * debugControls.glitchIntensity;
  cursorInfluence = 0.8 * debugControls.cursorInfluence;
  channelSeparation = 1.5 * debugControls.channelSeparation;
  distortionScale = 15.0 * debugControls.distortionScale;
});

// Toggle debug overlay with 'd' key
const handleKeydown = (e: KeyboardEvent) => {
  if (e.key === "d" || e.key === "D") {
    showDebug.value = !showDebug.value;
  }
};

// Add a new state variable to track mouse entry transition
const mouseEntryTransition = ref(0); // 0 = no influence, 1 = full influence

// Mouse movement handler - directly calculates normalized positions (0-1) within SVG bounds
// No velocity or smoothing calculations, for immediate visual response to cursor movement
const handleMouseMove = (event: MouseEvent) => {
  rawMouseX.value = event.clientX;
  rawMouseY.value = event.clientY;
  
  // Check if mouse is over the SVG and calculate normalized position in one step
  const svgRect = svg.value?.getBoundingClientRect();
  const wasOverSvg = isMouseOverSvg.value;
  
  if (svgRect && svgRect.width > 0 && svgRect.height > 0) {
    // Calculate normalized position directly (0-1 range)
    normMouseX.value = Math.max(0, Math.min(1, (event.clientX - svgRect.left) / svgRect.width));
    normMouseY.value = Math.max(0, Math.min(1, (event.clientY - svgRect.top) / svgRect.height));
    
    // Check if mouse is over SVG
    isMouseOverSvg.value = (
      event.clientX >= svgRect.left &&
      event.clientX <= svgRect.right &&
      event.clientY >= svgRect.top &&
      event.clientY <= svgRect.bottom
    );
  } else {
    isMouseOverSvg.value = false;
    // Keep last position if not over SVG
  }
  
  // If mouse just entered the SVG, start transition from 0
  if (!wasOverSvg && isMouseOverSvg.value) {
    mouseEntryTransition.value = 0;
  }
  
  if (!mouseHasMoved.value) {
    mouseHasMoved.value = true;
  }
};

// Helper function to calculate distance-based influence using a parabolic fall-off
// Returns 1.0 at the cursor position and falls off quadratically to 0 at the specified radius
const getDistanceBasedInfluence = (
  normX: number, 
  normY: number, 
  cursorNormX: number, 
  cursorNormY: number, 
  radius: number
): number => {
  const dx = normX - cursorNormX;
  const dy = normY - cursorNormY;
  const distSquared = dx * dx + dy * dy;
  const radiusSquared = radius * radius;
  
  // If beyond radius, no influence
  if (distSquared > radiusSquared) {
    return 0;
  }
  
  // Sharper parabolic fall-off: square the term so it drops off faster
  const baseInfluence = 1 - distSquared / radiusSquared;
  // Raise to the 3rd power for an even steeper drop-off outside the focus radius
  return baseInfluence <= 0 ? 0 : Math.pow(baseInfluence, 3);
};

// --- Main Animation Loop ---
const animate = () => {
  const innerAnimate = () => {
    // Only increment time variables if master effect is enabled
    if (masterEffectEnabled) {
      wobbleTime += 0.025;
      seedCounter.value += 0.05;
    }

    // Decay the click glitch effect over time
    if (clickGlitchIntensity.value > 0) {
      clickGlitchIntensity.value *= clickGlitchDecay;

      // Reset to zero if it gets too small
      if (clickGlitchIntensity.value < 0.01) {
        clickGlitchIntensity.value = 0;
      }
    }

    // Animate mouse entry transition
    if (isMouseOverSvg.value && mouseEntryTransition.value < 1) {
      mouseEntryTransition.value = Math.min(1, mouseEntryTransition.value + 0.05); // Smooth transition in
    } else if (!isMouseOverSvg.value && mouseEntryTransition.value > 0) {
      mouseEntryTransition.value = Math.max(0, mouseEntryTransition.value - 0.05); // Smooth transition out
    }

    // Mouse position is already normalized in handleMouseMove - direct input without smoothing
    // This provides immediate visual response to cursor movements without velocity calculations
    
    // Use center position if mouse is outside SVG or hasn't moved
    const effectiveMouseX = (isMouseOverSvg.value && mouseHasMoved.value) ? normMouseX.value : 0.5;
    const effectiveMouseY = (isMouseOverSvg.value && mouseHasMoved.value) ? normMouseY.value : 0.5;

    // Calculate distance from center (normalized 0-1)
    const distanceFromCenter = Math.sqrt(
      Math.pow(effectiveMouseX - 0.5, 2) + 
      Math.pow(effectiveMouseY - 0.5, 2)
    ) * 1.414; // Scale by sqrt(2) to normalize to 0-1 range
    
    // Calculate center proximity factor (1 at center, approaches 0 at edges)
    const centerProximityFactor = Math.max(0, 1 - distanceFromCenter);

    // --- Simplified time calculation without cursor modulation ---
    // The cursor should affect the output, not the time progression itself
    // This prevents accumulating phase drift over time

    // Apply entry transition for smooth mouse entry
    const transitionedCursorInfluence = cursorInfluence * mouseEntryTransition.value * 0.3; // Further reduced

    // Boost effects when click is active
    const clickBoost = 1.0 + (clickGlitchIntensity.value * 1.0); // Reduced click boost

    // Use unmodulated wobble time to prevent phase accumulation
    // Only apply a simple scale based on hover state, not cursor position
    const autonomousWobbleScale = isMouseOverSvg.value ? 0.7 : 1.0; // Slower when hovering
    const effectiveWobbleTime = wobbleTime * autonomousWobbleScale * clickBoost;

    // Calculate cursor influence separately - this will be applied to displacement, not time
    const cursorDisplacementInfluence = transitionedCursorInfluence * centerProximityFactor;
    
    // --- Parabolic Scanline Emanation from Cursor ---
    // Create a parabolic field that originates from the cursor position
    // The effect is minimal at the cursor and increases parabolically outward

    // Calculate distance from cursor for radial effect
    const distFromCursorX = effectiveMouseX - 0.5;
    const distFromCursorY = effectiveMouseY - 0.5;
    const radialDistance = Math.sqrt(distFromCursorX * distFromCursorX + distFromCursorY * distFromCursorY);

    // Parabolic influence that's 0 at cursor, 1 at edges
    // This creates the "eye of the storm" effect at cursor position
    const parabolaRadius = 0.4; // Effect radius (40% of screen)
    const parabolicDistortion = radialDistance <= parabolaRadius
      ? Math.pow(radialDistance / parabolaRadius, 2) // Parabolic increase from center
      : 1.0; // Full effect outside radius

    // Directional parabolas for X and Y axes
    // These control how the scanlines bend away from cursor
    const xDistFromCursor = Math.abs(distFromCursorX);
    const yDistFromCursor = Math.abs(distFromCursorY);

    // Inverted parabolas - minimal at cursor, maximum away
    const axisParabolaX = Math.min(1, Math.pow(xDistFromCursor * 2, 2));
    const axisParabolaY = Math.min(1, Math.pow(yDistFromCursor * 2, 2));

    // Scanline emanation strength based on distance from cursor
    const scanlineEmanation = parabolicDistortion;
    const scanlineFrequencyModulation = 1.0 + radialDistance * 0.5; // Frequency increases away from cursor

    // --- Apply default values for all channels when effects are disabled ---
    if (!masterEffectEnabled) {
      // Set all effects to 0 for a static display
      for (let i = 0; i < 3; i++) {
        const turbulence = turbulenceRefs.value[i];
        const displacement = displacementRefs.value[i];
        const offset = offsetRefs.value[i];
        const colorMatrix = colorMatrixRefs.value[i];
        
        if (turbulence) {
          turbulence.setAttributeNS(null, "baseFrequency", "0.0001 0.0001");
          turbulence.setAttributeNS(null, "numOctaves", "1");
        }
        
        if (displacement) {
          displacement.setAttributeNS(null, "scale", "0");
        }
        
        if (offset) {
          offset.setAttributeNS(null, "dx", "0");
          offset.setAttributeNS(null, "dy", "0");
        }
        
        // Keep color matrices at full intensity
        if (colorMatrix) {
          const value = i === 0 ? "1 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 0 1 0" :
                       i === 1 ? "0 0 0 0 0  0 1 0 0 0  0 0 0 0 0  0 0 0 1 0" :
                       "0 0 0 0 0  0 0 0 0 0  0 0 1 0 0  0 0 0 1 0";
          colorMatrix.setAttributeNS(null, "values", value);
        }
      }
      
      // Schedule the next animation frame
      frameId.value = requestAnimationFrame(innerAnimate);
      return;
    }

    // --- Channel Drift Parameters ---
    // Apply global effect intensity to drift amplitude
    const effectiveDriftAmplitude = globalEffectIntensity <= 0 ? 0 : channelSeparation * globalEffectIntensity;
    
    // Mouse influence on drift (simplified to use centerProximityFactor)
    const mouseDriftInfluence = (globalEffectIntensity <= 0) ? 0 : 
      (isMouseOverSvg.value && mouseHasMoved.value) 
        ? effectiveMouseY * centerProximityFactor * globalEffectIntensity * cursorInfluence
        : 0.5 * centerProximityFactor * globalEffectIntensity; 
    
    // Add amplitude boost based on center proximity
    const centerAmplitudeBoost = (globalEffectIntensity <= 0) ? 0 :
      isMouseOverSvg.value 
        ? centerProximityFactor * 0.35 * cursorInfluence * centerProximityFactor
        : 0;
        
    // Suppress ambient movement while hovering
    const ambientFactor = isMouseOverSvg.value ? 0.05 * cursorInfluence : 1;
    const driftAmplitude = (globalEffectIntensity <= 0) ? 0 : 
      (effectiveDriftAmplitude + mouseDriftInfluence + centerAmplitudeBoost) * ambientFactor;

    // Apply Master Modulators to Continuous Effects (Wobble Amplitude)
    const baseMouseWobbleInfluence = (globalEffectIntensity <= 0 || !masterEffectEnabled) ? 0 :
      (isMouseOverSvg.value && mouseHasMoved.value) ? effectiveMouseY * 2.5 * centerProximityFactor : 0;
    const mouseWobbleInfluence = (globalEffectIntensity <= 0 || !masterEffectEnabled) ? 0 :
      baseMouseWobbleInfluence * globalEffectIntensity;
    
    // Incorporate center proximity
    const baseMouseDrivenWobbleAmplitude = (globalEffectIntensity <= 0 || !masterEffectEnabled) ? 0 :
      1.5 + (centerProximityFactor * 1.0);
    const mouseDrivenWobbleAmplitude = (globalEffectIntensity <= 0 || !masterEffectEnabled) ? 0 :
      (baseMouseDrivenWobbleAmplitude * globalEffectIntensity + mouseWobbleInfluence) * ambientFactor;

    // --- Simplified warp centering with fewer parameters
    const defaultNumOctaves = "3";
    const glitchNumOctaves = "1";
    const baseWarpCenteringFactor = (globalEffectIntensity <= 0 || !masterEffectEnabled) ? 0 : 5;
    
    // Simplified pulsation with fewer parameters  
    const pulsatingMultiplier = (globalEffectIntensity <= 0 || !masterEffectEnabled) ? 0 :
      0.75 + 0.25 * Math.cos(effectiveWobbleTime * 0.125);
    
    // Effective warp centering strength
    const effectiveWarpCenteringFactor = (globalEffectIntensity <= 0 || !masterEffectEnabled) ? 0 :
      baseWarpCenteringFactor * centerProximityFactor * pulsatingMultiplier;

    // --- Color Pulsation ---
    // Pulsates the intensity of each color channel using a slow sine wave.
    const colorPulseSpeed = 0.2;
    const colorPulseMin = 0.75;
    const colorPulseMax = 1.0;
    const colorIntensityPulse = (globalEffectIntensity <= 0 || !masterEffectEnabled) ? 1.0 :
      colorPulseMin +
      ((Math.sin(effectiveWobbleTime * colorPulseSpeed) + 1) / 2) *
        (colorPulseMax - colorPulseMin);

    // Color matrix values for each channel
    const colorMatrixValues = [
      `${colorIntensityPulse.toFixed(3)} 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 0 1 0`,
      `0 0 0 0 0  0 ${colorIntensityPulse.toFixed(3)} 0 0 0  0 0 0 0 0  0 0 0 1 0`,
      `0 0 0 0 0  0 0 0 0 0  0 0 ${colorIntensityPulse.toFixed(3)} 0 0  0 0 0 1 0`
    ];

    // Modify the glitch frequency and effects when click is active
    let effectiveGlitchFrequency = debugControls.glitchFrequency;
    let effectiveGlitchIntensity = baseGlitchIntensity;
    
    // Amplify glitch intensity based on click state - with reduced impact
    if (clickGlitchIntensity.value > 0) {
      // Very subtle increase in glitch frequency
      effectiveGlitchFrequency += clickGlitchIntensity.value * 0.02; // Further reduced from 0.05 to 0.02
      // More moderate increase in glitch intensity
      effectiveGlitchIntensity *= (1.0 + clickGlitchIntensity.value * 2.0); // Reduced from 4.0 to 2.0
    }
    
    // Glitch trigger probability and duration
    const glitchProbability = (globalEffectIntensity <= 0 || !masterEffectEnabled) ? 0 : 
      effectiveGlitchFrequency * globalEffectIntensity;
    const minGlitchDuration = 50;
    const maxGlitchDuration = 100;

    // Apply a more aggressive scaling to ensure small values have less impact
    const effectiveGlobalIntensity = (globalEffectIntensity <= 0 || !masterEffectEnabled) ? 0 : 
      Math.pow(globalEffectIntensity, 0.7);

    // Unified channel animation loop
    for (let i = 0; i < 3; i++) {
      // Get references to current channel's filter elements
      const turbulence = turbulenceRefs.value[i];
      const displacement = displacementRefs.value[i];
      const offset = offsetRefs.value[i];
      const colorMatrix = colorMatrixRefs.value[i];
      
      if (!turbulence || !displacement || !offset || !colorMatrix) {
        continue; // Skip if any ref is null
      }
      
      // Apply color matrix values
      colorMatrix.setAttributeNS(null, "values", colorMatrixValues[i]);
      
      // Randomly trigger glitches for this channel
      if (Math.random() < glitchProbability && glitchCounters.value[i] === 0) {
        glitchCounters.value[i] = minGlitchDuration + Math.floor(Math.random() * (maxGlitchDuration - minGlitchDuration + 1));
        spikeDirections.value[i] = Math.random() < 0.7 ? "horizontal" : "vertical";
      }
      
      // Reset glitch counters when effects are disabled
      if (globalEffectIntensity <= 0 || !masterEffectEnabled) {
        glitchCounters.value[i] = 0;
        spikeDirections.value[i] = null;
      }
      
      // Use phase offsets from config
      const phaseConfig = channelPhaseOffsets[i];
      
      // Calculate base drift for this channel
      // Use clean time without cursor modulation to prevent phase accumulation
      const driftIntensityMultiplier = (globalEffectIntensity <= 0 || !masterEffectEnabled) ? 0 : 1;
      const baseDriftX = Math.sin(
        effectiveWobbleTime * phaseConfig.driftX +
        (i === 0 ? 0 : i === 1 ? 1 : 3) // Phase offset specific to channel
      ) * driftAmplitude * driftIntensityMultiplier;

      const baseDriftY = Math.cos(
        effectiveWobbleTime * phaseConfig.driftY +
        (i === 0 ? 0 : i === 1 ? 2 : 4) // Phase offset specific to channel
      ) * driftAmplitude * driftIntensityMultiplier;
      
      // Per-channel wobble scales
      const baseWobbleScale = (globalEffectIntensity <= 0 || !masterEffectEnabled) ? 0 : distortionScale;
      const wobbleScale = baseWobbleScale + ((globalEffectIntensity <= 0 || !masterEffectEnabled) ? 0 :
        Math.sin(
          effectiveWobbleTime + phaseConfig.wobblePhase // Use clean time without cursor modulation
        ) * mouseDrivenWobbleAmplitude);
      
      // Final displacement values
      let finalDX = baseDriftX;
      let finalDY = baseDriftY;

      // Add scanline emanation effect - scanlines appear to radiate from cursor
      // The effect is minimal at cursor position and increases parabolically outward
      if (isMouseOverSvg.value && !glitchCounters.value[i]) {
        // Calculate direction from cursor to current position
        // This creates the "emanation" effect where scanlines bend away from cursor
        const directionX = distFromCursorX > 0 ? 1 : -1;
        const directionY = distFromCursorY > 0 ? 1 : -1;

        // Displacement increases with distance from cursor (parabolic)
        // Minimal at cursor, maximum at edges
        const scanlineBendX = directionX * axisParabolaX * scanlineEmanation * 2.0 * (i * 0.1 + 0.9);
        const scanlineBendY = directionY * axisParabolaY * scanlineEmanation * 1.0 * (i * 0.1 + 0.9);

        // Apply with reduced influence for subtlety
        finalDX += scanlineBendX * cursorDisplacementInfluence;
        finalDY += scanlineBendY * cursorDisplacementInfluence;
      }
      
      // Glitch state handling
      if (glitchCounters.value[i] > 0) {
        // Active glitch
        turbulence.setAttributeNS(null, "numOctaves", glitchNumOctaves);

      // Calculate cursor focus factor for localized glitch
        const cursorFocusRadius = 0.1 * cursorInfluence;
      let cursorFocusFactor = 0;
        
      if (isMouseOverSvg.value) {
          if (spikeDirections.value[i] === "horizontal") {
          cursorFocusFactor = getDistanceBasedInfluence(
              0.5, 0.5, 0.5, effectiveMouseY, cursorFocusRadius
            ) * 0.25 * cursorInfluence;
          } else {
          cursorFocusFactor = getDistanceBasedInfluence(
              0.5, 0.5, effectiveMouseX, 0.5, cursorFocusRadius
            ) * 0.25 * cursorInfluence;
          }
        }
        
        // Get glitch settings based on direction
        const isHorizontal = spikeDirections.value[i] === "horizontal";
        const freqSettings = isHorizontal 
          ? glitchFrequencySettings.horizontal 
          : glitchFrequencySettings.vertical;
        
        // Calculate frequency shifts based on wobble time (not cursor-modulated)
        // This prevents accumulation of phase offset over time
        const autoFreqShiftX = Math.sin(
          effectiveWobbleTime * (0.125 + 0.01 * i) // Use clean time without cursor modulation
        ) * (freqSettings.xAmpFactor * 0.6 * (1.0 + centerProximityFactor * 0.3));

        const autoFreqShiftY = Math.cos(
          effectiveWobbleTime * (0.15 + 0.01 * i) // Use clean time without cursor modulation
        ) * (freqSettings.yAmpFactor * 0.6 * (1.0 + centerProximityFactor * 0.3));
        
        // Calculate base frequencies
        const currentBaseFreqX = freqSettings.baseX + autoFreqShiftX;
        const currentBaseFreqY = freqSettings.baseY + autoFreqShiftY;
        
        // Calculate glitch scale
        let currentScale;
        if (isHorizontal) {
          const horizontalScaleMouseInfluence = (1 - Math.abs(effectiveMouseY - 0.5) * 2) * 20;
          currentScale = (12 + (horizontalScaleMouseInfluence * centerProximityFactor) + 
                         (centerProximityFactor * (6 + i)) + (cursorFocusFactor * 8)) * 0.6;
                           
          // Add localized displacement if in cursor focus area
        if (cursorFocusFactor > 0) {
            finalDX += Math.random() * 4 * cursorFocusFactor * centerProximityFactor;
          }
        } else {
          currentScale = (12 + effectiveMouseY * 20 * centerProximityFactor + 
                         (centerProximityFactor * (6 + i)) + (cursorFocusFactor * 8)) * 0.6;
                           
          // Add localized displacement if in cursor focus area
        if (cursorFocusFactor > 0) {
            finalDY += Math.random() * 4 * cursorFocusFactor * centerProximityFactor;
          }
        }
        
        // Use click position to create a directional effect for each channel
        // This makes glitches appear to be centered on or directed towards the click point
        if (clickGlitchIntensity.value > 0) {
          // Use click position to create a directional effect for each channel
          // This makes glitches appear to be centered on or directed towards the click point
          if (isHorizontal) {
            // For horizontal glitches, use click X position to influence scale - reduced multiplier
            const clickInfluence = 1.0 + Math.abs(clickLocationX.value - 0.5) * 4.0 * clickGlitchIntensity.value; // Reduced from 8.0 to 4.0
            currentScale *= clickInfluence;
            
            // Make jitter stronger at click location - reduced strength
            const distanceToClickY = Math.abs(clickLocationY.value - effectiveMouseY);
            if (distanceToClickY < 0.3) {
              const proximityFactor = (0.3 - distanceToClickY) / 0.3;
              finalDX += (Math.random() - 0.5) * 5 * proximityFactor * clickGlitchIntensity.value; // Reduced from 10 to 5
            }
    } else {
            // For vertical glitches, use click Y position to influence scale - reduced multiplier
            const clickInfluence = 1.0 + Math.abs(clickLocationY.value - 0.5) * 4.0 * clickGlitchIntensity.value; // Reduced from 8.0 to 4.0
            currentScale *= clickInfluence;
            
            // Make jitter stronger at click location - reduced strength
            const distanceToClickX = Math.abs(clickLocationX.value - effectiveMouseX);
            if (distanceToClickX < 0.3) {
              const proximityFactor = (0.3 - distanceToClickX) / 0.3;
              finalDY += (Math.random() - 0.5) * 5 * proximityFactor * clickGlitchIntensity.value; // Reduced from 10 to 5
            }
          }
        }
        
        // Set turbulence parameters
        turbulence.setAttributeNS(
        null,
          "baseFrequency", 
          `${Math.max(0.0001, currentBaseFreqX).toFixed(4)} ${Math.max(0.0001, currentBaseFreqY).toFixed(4)}`
        );
        
        // Set displacement scale
        displacement.setAttributeNS(null, "scale", `${Math.max(0, currentScale)}`);
        
        // Add jitter to final displacement
        const jitterX = (Math.random() - 0.5) * 2 * effectiveGlitchIntensity * 
                       effectiveMouseX * centerProximityFactor * globalEffectIntensity;
        const jitterY = (Math.random() - 0.5) * 2 * effectiveGlitchIntensity * 
                       effectiveMouseY * centerProximityFactor * globalEffectIntensity;
        finalDX += jitterX;
        finalDY += jitterY;
        
                  // Add directional warp based on cursor position
          // Adding (instead of subtracting) creates an effect where colors diverge outward from cursor
          // Only applied to green and blue channels to create chromatic separation
          if (i > 0) { // Only for green and blue channels
            const turbulenceShiftY = (0.5 + effectiveMouseY) * effectiveWarpCenteringFactor;
            const turbulenceShiftX = (0.5 + effectiveMouseX) * effectiveWarpCenteringFactor;
            finalDY += turbulenceShiftY;
            finalDX += turbulenceShiftX;
          }
        
        // Decrement glitch counter
        glitchCounters.value[i]--;
      } else {
        // Normal non-glitch state
        // Use 2 octaves for smoother scanline patterns with some detail
        const scanlineOctaves = isMouseOverSvg.value ? "2" : defaultNumOctaves;
        turbulence.setAttributeNS(null, "numOctaves", scanlineOctaves);
        turbulence.setAttributeNS(null, "seed", `${Math.floor(seedCounter.value)}`);
        
        // Get frequency settings for this channel
        const freqSettings = channelFrequencySettings[i];

        // Create radial scanline pattern emanating from cursor
        // Frequency is lowest at cursor position and increases parabolically outward
        // This creates the appearance of scanlines radiating from the cursor point

        // Base frequencies that increase with distance from cursor
        const scanlineBaseFreqX = 0.001 + (parabolicDistortion * 0.03); // Minimal at cursor, increases outward
        const scanlineBaseFreqY = 0.0001 + (parabolicDistortion * 0.0003); // Very low Y for horizontal scanlines

        // Add directional frequency modulation based on cursor position
        // This creates asymmetric patterns above/below cursor
        const directionalFreqX = scanlineBaseFreqX * (1.0 + Math.abs(distFromCursorY) * 0.5);
        const directionalFreqY = scanlineBaseFreqY * (1.0 + Math.abs(distFromCursorX) * 0.5);

        // Mix scanline frequencies with original turbulence
        // Stronger scanline effect when hovering
        const scanlineMixFactor = isMouseOverSvg.value ? 0.85 : 0.2; // Emphasize scanlines when hovering

        const defaultFreqX = Math.max(0.00001,
          globalEffectIntensity === 0 ? 0 :
          (freqSettings.baseX * (1 - scanlineMixFactor) + directionalFreqX * scanlineMixFactor) * globalEffectIntensity
        );
        const defaultFreqY = Math.max(0.00001,
          globalEffectIntensity === 0 ? 0 :
          (freqSettings.baseY * (1 - scanlineMixFactor) + directionalFreqY * scanlineMixFactor) * globalEffectIntensity
        );

        // Set turbulence parameters
        turbulence.setAttributeNS(
        null,
          "baseFrequency",
          `${defaultFreqX.toFixed(6)} ${defaultFreqY.toFixed(6)}`
        );

        // Set displacement scale with parabolic emanation from cursor
        // Scale is minimal at cursor position and increases parabolically outward
        const baseScale = globalEffectIntensity === 0 ? 0 : distortionScale * 0.2; // Base scale

        // Calculate emanation-based scale
        // Minimal displacement at cursor, maximum at edges (parabolic increase)
        const emanationScale = isMouseOverSvg.value
          ? baseScale * (0.1 + parabolicDistortion * 2.0) // Scale from 10% at cursor to 210% at edges
          : baseScale * 0.8;

        // Add subtle pulsation to make the emanation "breathe"
        const pulsation = 1.0 + Math.sin(effectiveWobbleTime * 0.3) * 0.1;

        // Apply the emanation scale with pulsation
        const effectiveWobbleScale = globalEffectIntensity === 0 ? 0 : emanationScale * pulsation;
        displacement.setAttributeNS(null, "scale", `${effectiveWobbleScale}`);
      }
      
      // Set final offsets
      offset.setAttributeNS(null, "dx", `${finalDX}`);
      offset.setAttributeNS(null, "dy", `${finalDY}`);
    }

    // Schedule the next animation frame
    frameId.value = requestAnimationFrame(innerAnimate);
  };
  
  // Start the animation loop
  innerAnimate();
};

// Click handler to trigger glitch and distortion effects
// Creates a temporary intense visual disturbance centered on click position
// The click effect decays gradually based on clickGlitchDecay parameter
const handleSvgClick = (event: MouseEvent) => {
  // Only allow click effect if clickable (prevents rapid repeated clicks)
  if (!isClickable.value) return;
  
  // Use direct normalized mouse position for precise click location
  // This results in distortion effects emanating exactly from click point
  clickLocationX.value = normMouseX.value;
  clickLocationY.value = normMouseY.value;
  
  // Force a glitch on all channels with a random duration
  const minDuration = 80; // Reduced from 100 to 80
  const maxDuration = 130; // Reduced from 180 to 130
  
  for (let i = 0; i < 3; i++) {
    // Longer duration for more dramatic effect
    glitchCounters.value[i] = minDuration + Math.floor(Math.random() * (maxDuration - minDuration));
    
    // Randomize direction, with different probabilities per channel for variety
    if (i === 0) {
      spikeDirections.value[i] = Math.random() < 0.8 ? "horizontal" : "vertical"; // Red: favor horizontal
    } else if (i === 1) {
      spikeDirections.value[i] = Math.random() < 0.5 ? "horizontal" : "vertical"; // Green: 50/50
    } else {
      spikeDirections.value[i] = Math.random() < 0.3 ? "horizontal" : "vertical"; // Blue: favor vertical
    }
  }
  
  // Set click glitch intensity to max
  clickGlitchIntensity.value = maxClickGlitchIntensity;
  
  // Add a smaller time jump for more subtle effect
  wobbleTime += 0.4; // Reduced from 0.8 to 0.4
  
  // Prevent click spam by temporarily disabling clicking
  isClickable.value = false;
  setTimeout(() => {
    isClickable.value = true;
  }, 400); // Reduced from 500 to 400ms
};

// --- Lifecycle Hooks ---
onMounted(() => {
  // Initialize mouse positions to screen center
  rawMouseX.value = window.innerWidth / 2;
  rawMouseY.value = window.innerHeight / 2;
  normMouseX.value = 0.5; // Center position
  normMouseY.value = 0.5; // Center position
  isMouseOverSvg.value = false; // Start with mouse not over SVG

  // Get the error code and message from route params
  errorCode.value = route.params.code?.toString() || '404';
  errorMessage.value = route.params.message?.toString() || 'Page not found';

  // Adjust SVG size based on viewport and text length
  adjustSvgSize();

  // Initialize color channel filter refs with safer casting
  const channelIds = ['red', 'green', 'blue'];
  turbulenceRefs.value = channelIds.map(id => {
    const element = document.querySelector(`#${id}Turbulence`);
    return element as unknown as SVGFETurbulenceElement;
  });
  
  offsetRefs.value = channelIds.map(id => {
    const element = document.querySelector(`#${id}Offset`);
    return element as unknown as SVGFEOffsetElement;
  });
  
  displacementRefs.value = channelIds.map(id => {
    const element = document.querySelector(`#${id}Displacement`);
    return element as unknown as SVGFEDisplacementMapElement;
  });
  
  colorMatrixRefs.value = channelIds.map(id => {
    const element = document.querySelector(`#${id}ColorMatrix`);
    return element as unknown as SVGFEColorMatrixElement;
  });

  // Add window resize listener for responsive adjustments
  window.addEventListener('resize', adjustSvgSize);

  // Add mouse move listener to track cursor position.
  window.addEventListener("mousemove", handleMouseMove);

  // Add keydown listener
  window.addEventListener('keydown', handleKeydown);

  // Add click handler to SVG element when it's available
  if (svg.value) {
    svg.value.addEventListener('click', handleSvgClick);
  }

  // Start the animation.
  animate();
});

onUnmounted(() => {
  // Clean up: remove event listeners.
  window.removeEventListener("mousemove", handleMouseMove);
  window.removeEventListener('resize', adjustSvgSize);
  
  // Clean up: cancel the animation frame request if it exists.
  if (frameId.value) {
    cancelAnimationFrame(frameId.value);
  }

  // Remove keydown listener
  window.removeEventListener('keydown', handleKeydown);

  // Remove click event listener
  if (svg.value) {
    svg.value.removeEventListener('click', handleSvgClick);
  }
});

// Function to adjust SVG size based on viewport size and text content
const adjustSvgSize = () => {
  // Get viewport dimensions
  const viewportWidth = window.innerWidth;
  const viewportHeight = window.innerHeight;
  
  // Get text length (number of characters in error code)
  const textLength = errorCode.value.length;
  
  // Base SVG width on viewport width
  // For mobile: use nearly full width
  // For desktop: use proportional width
  const baseWidth = viewportWidth < 768 
    ? Math.min(viewportWidth * 0.9, 500) 
    : Math.min(viewportWidth * 0.6, 1000);
  
  // Adjust width further based on text length
  // Standard length is 3 chars (e.g., 404, 500)
  // For each additional character, increase width
  const widthAdjustment = textLength > 3 
    ? 1 + ((textLength - 3) * 0.15) // Each extra char adds 15% width
    : 1;
    
  // Set SVG width with adjustment
  const finalWidth = baseWidth * widthAdjustment;
  svgWidth.value = `${finalWidth}px`;
  
  // Height should maintain a good aspect ratio
  // Taller for mobile, wider for desktop
  const aspectRatio = viewportWidth < 768 ? 2 : 2.5;
  svgHeight.value = `${finalWidth / aspectRatio}px`;
  
  // Font size should be proportional to SVG height
  // but with a maximum size to prevent enormous text
  const maxFontPercentage = viewportWidth < 768 ? 0.6 : 0.7; // Font takes up to 60-70% of height
  const calculatedFontSize = (finalWidth / aspectRatio) * maxFontPercentage;
  
  // Adjust font size inversely with text length to fit longer text
  const fontSizeAdjustment = textLength > 3 
    ? 1 / (1 + ((textLength - 3) * 0.1)) // Each extra char reduces font by ~10%
    : 1;
    
  fontSize.value = `${calculatedFontSize * fontSizeAdjustment}px`;
};

// Per-control slider bounds
const debugMeta = computed(() => ({
  masterEffectEnabled: { min: 0, max: 1, step: 1 },
  globalEffectIntensity: { min: 0, max: 1, step: 0.01 },
  channelSeparation: { min: 0, max: 3, step: 0.1 },
  distortionScale: { min: 0, max: 30, step: 1 },
  glitchFrequency: { min: 0, max: 0.5, step: 0.01 },
  glitchIntensity: { min: 0, max: 2, step: 0.01 },
  cursorInfluence: { min: 0, max: 2, step: 0.01 }
}));
</script>

<template>
  <div
    class="min-h-screen w-full flex items-center justify-center bg-app p-4 select-none"
  >
    <div class="flex flex-col text-center">
      <svg ref="svg" class="error-svg" :width="svgWidth" :height="svgHeight">
        <defs>
          <filter id="rgbGlitch" primitiveUnits="userSpaceOnUse">
            <!-- 
              SVG Filter Chain Explanation:
              The filter works by creating three distorted versions of the source text (one for each RGB channel)
              and then blending them together. Each channel's distortion is driven by an feTurbulence (noise)
              which is then offset and used by an feDisplacementMap. The feColorMatrix isolates the specific channel.

              The final output is achieved after these steps for each of Red, Green, and Blue channels:
              1. feTurbulence: Generates Perlin noise. Key attributes like `baseFrequency`, `numOctaves`, and `seed` are dynamically animated.
                 - `baseFrequency`: Controls the scale/granularity of the noise. Lower values = larger, smoother patterns.
                 - `numOctaves`: Controls the detail/complexity of the noise. Higher values = more detail.
                 - `seed`: Changes the noise pattern entirely.
              2. feOffset: Shifts the generated turbulence texture. Used for the broad warp centering effect (controlled by smoothed mouse position)
                 and for the continuous drift of channels.
              3. feDisplacementMap: Uses the (offset) turbulence noise to distort the `SourceGraphic` (the original text).
                 - `scale`: Controls the intensity/amplitude of the displacement.
              4. feColorMatrix: Isolates the color for the current channel (e.g., makes everything red for the red channel path).
                 Its `values` attribute is also pulsed slightly to make colors breathe.
              5. feBlend (mode="screen"): Combines the R, G, and B distorted layers additively to create the final RGB glitch effect.
            -->

            <!-- Red Channel -->
            <feTurbulence
              id="redTurbulence"
              type="turbulence"
              baseFrequency="0.01"
              numOctaves="3"
              result="redTurb"
            />
            <feOffset
              id="redOffset"
              in="redTurb"
              dx="0"
              result="redOffsetTurb"
            />
            <feDisplacementMap
              id="redDisplacement"
              in="SourceGraphic"
              in2="redOffsetTurb"
              scale="15"
              xChannelSelector="R"
              yChannelSelector="G"
              result="redDistorted"
            />
            <feColorMatrix
              id="redColorMatrix"
              in="redDistorted"
              type="matrix"
              values="1 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 0 1 0"
              result="red"
            />

            <!-- Green Channel -->
            <feTurbulence
              id="greenTurbulence"
              type="turbulence"
              baseFrequency="0.01"
              numOctaves="3"
              result="greenTurb"
            />
            <feOffset
              id="greenOffset"
              in="greenTurb"
              dx="0"
              result="greenOffsetTurb"
            />
            <feDisplacementMap
              id="greenDisplacement"
              in="SourceGraphic"
              in2="greenOffsetTurb"
              scale="15"
              xChannelSelector="R"
              yChannelSelector="G"
              result="greenDistorted"
            />
            <feColorMatrix
              id="greenColorMatrix"
              in="greenDistorted"
              type="matrix"
              values="0 0 0 0 0  0 1 0 0 0  0 0 0 0 0  0 0 0 1 0"
              result="green"
            />

            <!-- Blue Channel -->
            <feTurbulence
              id="blueTurbulence"
              type="turbulence"
              baseFrequency="0.01"
              numOctaves="3"
              result="blueTurb"
            />
            <feOffset
              id="blueOffset"
              in="blueTurb"
              dx="0"
              result="blueOffsetTurb"
            />
            <feDisplacementMap
              id="blueDisplacement"
              in="SourceGraphic"
              in2="blueOffsetTurb"
              scale="15"
              xChannelSelector="R"
              yChannelSelector="G"
              result="blueDistorted"
            />
            <feColorMatrix
              id="blueColorMatrix"
              in="blueDistorted"
              type="matrix"
              values="0 0 0 0 0  0 0 0 0 0  0 0 1 0 0  0 0 0 1 0"
              result="blue"
            />

            <!-- Additive Blending with feBlend -->
            <feBlend in="red" in2="green" mode="screen" result="redGreen" />
            <feBlend
              in="redGreen"
              in2="blue"
              mode="screen"
              result="finalGlitchOutput"
            />

          </filter>
          
        </defs>
        <text
          x="50%"
          y="50%"
          text-anchor="middle"
          dominant-baseline="middle"
          :filter="masterEffectEnabled ? 'url(#rgbGlitch)' : 'none'"
          class="error-text"
          :style="{ fontSize: fontSize }"
        >
          {{ errorCode }}
        </text>
      </svg>
      <div class="flex flex-col gap-4">
        <div class="text-2xl text-secondary">
          {{ errorMessage }}
        </div>

        <p class="mt-2 text-tertiary">
          The page you're looking for doesn't exist or you may not have access to it.
        </p>

        <div class="mt-8 flex gap-4 justify-center">
          <button
            @click="goBack"
            class="px-4 py-2 text-sm font-medium text-secondary hover:text-primary transition-colors"
          >
            &larr; Go back
          </button>
          <button
            @click="goHome"
            class="px-4 py-2 text-sm font-medium bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors"
          >
            Go to Dashboard
          </button>
        </div>
      </div>
    </div>
  </div>
  <!-- Debug Panel -->
  <div v-if="showDebug" class="fixed top-4 right-4 bg-surface/90 text-sm text-secondary p-4 rounded-lg max-h-[90vh] overflow-auto flex flex-col gap-3 z-50 shadow-lg">
    <h3 class="font-semibold mb-2">Debug Controls (press 'd' to toggle)</h3>
    
    <!-- Master Toggle -->
    <div class="flex items-center justify-between mb-4 border-b border-slate-600 pb-2">
      <span class="font-semibold">Master Effects Toggle</span>
      <input
        type="checkbox"
        v-model="debugControls.masterEffectEnabled"
        class="w-4 h-4"
      />
    </div>
    
    <!-- Main Effect Controls -->
    <div class="flex flex-col gap-3">
      <!-- Global Intensity -->
      <div class="flex flex-col gap-1">
        <label class="flex justify-between items-center gap-2">
          <span class="font-semibold">Global Intensity</span>
          <span class="tabular-nums w-12 text-right">{{ debugControls.globalEffectIntensity.toFixed(2) }}</span>
        </label>
        <input
          type="range"
          v-model.number="debugControls.globalEffectIntensity"
          :min="debugMeta.globalEffectIntensity.min"
          :max="debugMeta.globalEffectIntensity.max"
          :step="debugMeta.globalEffectIntensity.step"
          class="w-full"
        />
      </div>
      
      <!-- Channel Separation -->
      <div class="flex flex-col gap-1">
        <label class="flex justify-between items-center gap-2">
          <span>Channel Separation</span>
          <span class="tabular-nums w-12 text-right">{{ debugControls.channelSeparation.toFixed(2) }}</span>
        </label>
        <input
          type="range"
          v-model.number="debugControls.channelSeparation"
          :min="debugMeta.channelSeparation.min"
          :max="debugMeta.channelSeparation.max"
          :step="debugMeta.channelSeparation.step"
          class="w-full"
        />
      </div>
      
      <!-- Distortion Scale -->
      <div class="flex flex-col gap-1">
        <label class="flex justify-between items-center gap-2">
          <span>Distortion Scale</span>
          <span class="tabular-nums w-12 text-right">{{ debugControls.distortionScale.toFixed(2) }}</span>
        </label>
        <input
          type="range"
          v-model.number="debugControls.distortionScale"
          :min="debugMeta.distortionScale.min"
          :max="debugMeta.distortionScale.max"
          :step="debugMeta.distortionScale.step"
          class="w-full"
        />
      </div>
      
      <!-- Glitch Frequency -->
      <div class="flex flex-col gap-1">
        <label class="flex justify-between items-center gap-2">
          <span>Glitch Frequency</span>
          <span class="tabular-nums w-12 text-right">{{ debugControls.glitchFrequency.toFixed(2) }}</span>
        </label>
        <input
          type="range"
          v-model.number="debugControls.glitchFrequency"
          :min="debugMeta.glitchFrequency.min"
          :max="debugMeta.glitchFrequency.max"
          :step="debugMeta.glitchFrequency.step"
          class="w-full"
        />
      </div>
      
      <!-- Glitch Intensity -->
      <div class="flex flex-col gap-1">
        <label class="flex justify-between items-center gap-2">
          <span>Glitch Intensity</span>
          <span class="tabular-nums w-12 text-right">{{ debugControls.glitchIntensity.toFixed(2) }}</span>
        </label>
        <input
          type="range"
          v-model.number="debugControls.glitchIntensity"
          :min="debugMeta.glitchIntensity.min"
          :max="debugMeta.glitchIntensity.max"
          :step="debugMeta.glitchIntensity.step"
          class="w-full"
        />
      </div>
      
      <!-- Cursor Influence -->
      <div class="flex flex-col gap-1">
        <label class="flex justify-between items-center gap-2">
          <span>Cursor Influence</span>
          <span class="tabular-nums w-12 text-right">{{ debugControls.cursorInfluence.toFixed(2) }}</span>
        </label>
        <input
          type="range"
          v-model.number="debugControls.cursorInfluence"
          :min="debugMeta.cursorInfluence.min"
          :max="debugMeta.cursorInfluence.max"
          :step="debugMeta.cursorInfluence.step"
          class="w-full"
        />
      </div>
    </div>
  </div>
</template>

<style scoped>
.error-svg {
  max-width: 100%;
  transition: transform 0.2s ease-out;
}

.error-svg:hover {
  transform: scale(1.02); /* Subtle scale increase on hover */
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

.error-svg {
  animation: float 6s ease-in-out infinite;
}

/* Error text styling */
.error-text {
  fill: white;
}
</style>
