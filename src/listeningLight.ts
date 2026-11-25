export const LIGHT_WIDTH = 58;
export const LIGHT_HEIGHT = 32;

const TURN = 2 * Math.PI;
const SAMPLES = 72;
const CONTOURS = [
  { phase: 0.3, rate: 0.89, offset: -3.5, tilt: -0.11, width: 0.8, opacity: 0.55 },
  { phase: 1.6, rate: 1.07, offset: 3, tilt: 0.16, width: 0.92, opacity: 0.64 },
  { phase: 3.4, rate: 0.73, offset: 0.5, tilt: -0.03, width: 0.67, opacity: 0.38 },
  { phase: 4.5, rate: 1.19, offset: -1.4, tilt: 0.07, width: 0.62, opacity: 0.34 },
];

export function startListeningLight(
  canvas: HTMLCanvasElement,
  getState: () => string,
  getAnalyser: () => AnalyserNode | undefined,
) {
  const canvasContext = canvas.getContext("2d");
  if (!canvasContext) return () => {};
  const context = canvasContext;

  const ratio = Math.min(window.devicePixelRatio || 1, 2);
  canvas.width = Math.round(LIGHT_WIDTH * ratio);
  canvas.height = Math.round(LIGHT_HEIGHT * ratio);
  context.scale(canvas.width / LIGHT_WIDTH, canvas.height / LIGHT_HEIGHT);
  context.lineJoin = "round";

  const preference = window.matchMedia("(prefers-reduced-motion: reduce)");
  let signal = new Float32Array(0);
  let frame = 0;
  let lastFrame: number | undefined;
  let time = 0;
  let level = 0;
  let speed = 0.7;

  function readVoice() {
    const analyser = getAnalyser();
    if (!analyser) return 0;
    if (signal.length !== analyser.fftSize) signal = new Float32Array(analyser.fftSize);
    analyser.getFloatTimeDomainData(signal);
    let energy = 0;
    for (const sample of signal) energy += sample * sample;
    const decibels = 10 * Math.log10(energy / signal.length);
    // Display quiet speech without changing the signal sent to transcription.
    return Math.min(1, Math.max(0, (decibels + 66) / 43));
  }

  function tick(now: number) {
    const dt = Math.min((now - (lastFrame ?? now)) / 1000, 0.05);
    lastFrame = now;
    const state = getState();
    const working = state === "processing" || state === "cleanup" || state === "inserting";
    const target = state === "recording" ? readVoice() : working ? 0.29 : 0.1;
    const response = target > level ? 0.034 : 0.23;
    level += (target - level) * (1 - Math.exp(-dt / response));
    const targetSpeed = working ? 1.25 : 0.63 + 0.85 * level;
    speed += (targetSpeed - speed) * (1 - Math.exp(-dt / 0.14));
    time += dt * speed;
    paintLight(context, time, level);
    frame = window.requestAnimationFrame(tick);
  }

  function syncMotion() {
    window.cancelAnimationFrame(frame);
    lastFrame = undefined;
    if (preference.matches) {
      paintLight(context, 1.6, 0.18);
    } else {
      paintLight(context, time, level);
      frame = window.requestAnimationFrame(tick);
    }
  }

  syncMotion();
  preference.addEventListener("change", syncMotion);
  return () => {
    window.cancelAnimationFrame(frame);
    preference.removeEventListener("change", syncMotion);
  };
}

function paintLight(context: CanvasRenderingContext2D, time: number, level: number) {
  const centerX = LIGHT_WIDTH / 2;
  const centerY = LIGHT_HEIGHT / 2;
  context.clearRect(0, 0, LIGHT_WIDTH, LIGHT_HEIGHT);
  context.globalCompositeOperation = "lighter";

  for (const contour of CONTOURS) {
    const { phase, rate, offset, tilt, width, opacity } = contour;
    const drift = time * rate;
    const separation = 0.82 + level * 0.56;
    const swell = 0.65 + level * 0.8;
    const centerShift = Math.sin(drift * 0.63 + phase) * 1.25;

    context.beginPath();
    for (let step = 0; step < SAMPLES; step++) {
      const angle = step * TURN / SAMPLES;
      // The shared traveling harmonics keep nearby edges moving together.
      const surface = Math.sin(3 * angle - time * 1.14 + phase * 0.23)
        + 0.43 * Math.cos(5 * angle + time * 0.68 - phase * 0.4);
      const ripple = Math.sin(2 * angle + drift * 0.86 + phase);
      const fine = Math.sin(4 * angle - drift * 1.16 + phase * 1.7);
      const x = (16.5 + swell * (2.3 * surface + 1.3 * ripple)) * Math.cos(angle);
      const y = (10.3 + swell * (1.3 * surface + 0.95 * ripple)) * Math.sin(angle)
        + swell * fine * 0.8;
      const px = centerX + x + offset * separation + centerShift;
      const py = centerY + y + x * tilt + Math.sin(drift * 0.7 + phase) * separation;
      if (step === 0) context.moveTo(px, py);
      else context.lineTo(px, py);
    }
    context.closePath();

    // A moving highlight band follows each edge and concentrates at crossings.
    const direction = drift * 0.59 + phase;
    const dx = Math.cos(direction) * 22;
    const dy = Math.sin(direction) * 12;
    const light = context.createLinearGradient(centerX - dx, centerY - dy, centerX + dx, centerY + dy);
    light.addColorStop(0, "#686c37");
    light.addColorStop(0.32, "#9c863e");
    light.addColorStop(0.55, "#e1b96f");
    light.addColorStop(0.66, "#fff0ce");
    light.addColorStop(0.78, "#c59955");
    light.addColorStop(1, "#777d49");

    context.strokeStyle = light;
    context.lineWidth = width + 1.6 + level * 0.45;
    context.globalAlpha = opacity * (0.14 + 0.06 * level);
    context.stroke();
    context.lineWidth = width + 0.27 * level;
    context.globalAlpha = opacity * (0.82 + 0.31 * level);
    context.stroke();
  }
}
