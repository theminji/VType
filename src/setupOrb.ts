export type SetupPhase = "intro" | "installing" | "ready" | "error";
export type SamplePhase = "idle" | "listening" | "text";

export interface StudyState {
  phase: SetupPhase;
  stage: number;
  sample: SamplePhase;
}

const vertex = `
  attribute vec2 position;
  varying vec2 uv;
  void main() {
    uv = position * 0.5 + 0.5;
    gl_Position = vec4(position, 0.0, 1.0);
  }
`;

const fragment = `
  precision mediump float;
  varying vec2 uv;
  uniform sampler2D artwork;
  uniform vec2 cover;
  uniform vec2 pointer;
  uniform vec4 flow;
  uniform float time;

  void main() {
    vec2 p = (uv - 0.5) * cover;
    float distanceToLens = length(p * vec2(1.12, 1.0));
    float influence = 1.0 - smoothstep(0.07, 0.48, distanceToLens);
    float wave = sin(p.y * 18.0 - time * 1.55 + flow.y);
    float crossWave = cos(p.x * 21.0 + p.y * 8.0 + time * 1.14);
    float bend = 0.008 + flow.x * 0.022;
    vec2 current = vec2(wave + crossWave * 0.35, crossWave * 0.7 - wave * 0.24);
    p += influence * (current * bend - pointer * 0.025);
    p.y *= 1.0 + influence * flow.z * 0.13;
    vec3 color = texture2D(artwork, 0.5 + p).rgb;
    float refraction = pow(max(0.0, sin(11.0 * p.y + 7.0 * p.x - time * 1.5 + flow.y)), 5.0);
    color += color * influence * refraction * (0.11 + flow.x * 0.34);
    color *= 1.0 - flow.w * 0.25;
    gl_FragColor = vec4(color, 1.0);
  }
`;

function compile(gl: WebGLRenderingContext, kind: number, source: string) {
  const shader = gl.createShader(kind);
  if (!shader) return null;
  gl.shaderSource(shader, source);
  gl.compileShader(shader);
  if (gl.getShaderParameter(shader, gl.COMPILE_STATUS)) return shader;
  gl.deleteShader(shader);
  return null;
}

export function startSetupOrb(
  canvas: HTMLCanvasElement,
  image: HTMLImageElement,
  getState: () => StudyState,
) {
  const context = canvas.getContext("webgl", { alpha: false, antialias: false });
  if (!context) return;
  const gl = context;

  const program = gl.createProgram();
  const vertexShader = compile(gl, gl.VERTEX_SHADER, vertex);
  const fragmentShader = compile(gl, gl.FRAGMENT_SHADER, fragment);
  const buffer = gl.createBuffer();
  const texture = gl.createTexture();
  if (!program || !vertexShader || !fragmentShader || !buffer || !texture) return;
  gl.attachShader(program, vertexShader);
  gl.attachShader(program, fragmentShader);
  gl.linkProgram(program);
  gl.deleteShader(vertexShader);
  gl.deleteShader(fragmentShader);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) return;
  gl.useProgram(program);
  gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);
  const position = gl.getAttribLocation(program, "position");
  gl.enableVertexAttribArray(position);
  gl.vertexAttribPointer(position, 2, gl.FLOAT, false, 0, 0);
  gl.bindTexture(gl.TEXTURE_2D, texture);
  gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, true);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);

  const cover = gl.getUniformLocation(program, "cover");
  const pointer = gl.getUniformLocation(program, "pointer");
  const state = gl.getUniformLocation(program, "flow");
  const clock = gl.getUniformLocation(program, "time");
  const motion = matchMedia("(prefers-reduced-motion: reduce)");
  const flow = [0, 0, 0, 0];
  const aim = [0, 0];
  const cursor = [0, 0];
  let hasImage = false;
  let frame = 0;
  let previous = 0;
  let elapsed = 0;

  function paint() {
    if (!hasImage || !canvas.width || !canvas.height) return;
    gl.uniform2f(pointer, cursor[0], cursor[1]);
    gl.uniform4f(state, flow[0], flow[1], flow[2], flow[3]);
    gl.uniform1f(clock, elapsed);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
    canvas.dataset.rendered = "true";
  }

  function update(dt = 1) {
    const { phase, stage, sample } = getState();
    const target = [
      sample === "listening" ? 1 : phase === "installing" ? 0.67 : 0.15,
      stage * 0.9 + (sample === "listening" ? 1.5 : 0),
      sample === "text" || phase === "ready" ? 1 : 0,
      phase === "error" ? 1 : 0,
    ];
    const ease = motion.matches ? 1 : 1 - Math.exp(-dt * 5.8);
    target.forEach((value, index) => { flow[index] += (value - flow[index]) * ease; });
    aim.forEach((value, index) => { cursor[index] += ((motion.matches ? 0 : value) - cursor[index]) * ease; });
    if (motion.matches) elapsed = 0;
    else elapsed += dt * (0.55 + flow[0] * 0.7);
    paint();
  }

  function resize() {
    if (!hasImage) return;
    const scale = Math.min(devicePixelRatio || 1, 1.5);
    canvas.width = Math.round(canvas.clientWidth * scale);
    canvas.height = Math.round(canvas.clientHeight * scale);
    if (!canvas.height || !canvas.width) return;
    const ratio = canvas.width / canvas.height;
    const textureRatio = image.naturalWidth / image.naturalHeight;
    gl.viewport(0, 0, canvas.width, canvas.height);
    gl.uniform2f(cover, Math.min(1, ratio / textureRatio), Math.min(1, textureRatio / ratio));
    paint();
  }

  function tick(now: number) {
    const dt = previous ? (now - previous) / 1000 : 0.034;
    previous = now;
    update(Math.min(dt, 0.045));
    frame = requestAnimationFrame(tick);
  }

  function sync() {
    cancelAnimationFrame(frame);
    previous = 0;
    if (!hasImage || document.hidden) return;
    if (motion.matches) update();
    else frame = requestAnimationFrame(tick);
  }

  function load() {
    if (!image.naturalWidth) return;
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, image);
    hasImage = true;
    resize();
    sync();
  }

  function move(event: PointerEvent) {
    const bounds = canvas.getBoundingClientRect();
    aim[0] = (event.clientX - bounds.left) / bounds.width - 0.5;
    aim[1] = 0.5 - (event.clientY - bounds.top) / bounds.height;
  }
  function reset() { aim[0] = 0; aim[1] = 0; }
  function loseContext() {
    delete canvas.dataset.rendered;
    hasImage = false;
    sync();
  }

  const size = new ResizeObserver(resize);
  size.observe(canvas);
  const panel = canvas.parentElement;
  panel?.addEventListener("pointermove", move);
  panel?.addEventListener("pointerleave", reset);
  canvas.addEventListener("webglcontextlost", loseContext);
  image.addEventListener("load", load);
  motion.addEventListener("change", sync);
  document.addEventListener("visibilitychange", sync);
  if (image.complete) load();

  return {
    update() { if (motion.matches) update(); },
    stop() {
      cancelAnimationFrame(frame);
      size.disconnect();
      panel?.removeEventListener("pointermove", move);
      panel?.removeEventListener("pointerleave", reset);
      canvas.removeEventListener("webglcontextlost", loseContext);
      image.removeEventListener("load", load);
      motion.removeEventListener("change", sync);
      document.removeEventListener("visibilitychange", sync);
      delete canvas.dataset.rendered;
      gl.deleteTexture(texture);
      gl.deleteBuffer(buffer);
      gl.deleteProgram(program);
    },
  };
}
