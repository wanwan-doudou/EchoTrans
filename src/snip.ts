import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

const overlay = document.getElementById("overlay") as HTMLDivElement;
const hint = document.getElementById("hint") as HTMLDivElement;
const box = document.getElementById("box") as HTMLDivElement;
const sizeLabel = document.getElementById("size_label") as HTMLSpanElement;

let dragging = false;
let startX = 0;
let startY = 0;

function reset() {
  dragging = false;
  box.classList.add("hidden");
  hint.classList.remove("hidden");
  overlay.classList.remove("selecting");
}

function cancel() {
  reset();
  void getCurrentWindow().hide();
}

function rectFrom(cx: number, cy: number) {
  const left = Math.min(startX, cx);
  const top = Math.min(startY, cy);
  const width = Math.abs(cx - startX);
  const height = Math.abs(cy - startY);
  return { left, top, width, height };
}

function updateBox(cx: number, cy: number) {
  const r = rectFrom(cx, cy);
  box.style.left = `${r.left}px`;
  box.style.top = `${r.top}px`;
  box.style.width = `${r.width}px`;
  box.style.height = `${r.height}px`;

  const dpr = window.devicePixelRatio || 1;
  sizeLabel.textContent = `${Math.round(r.width * dpr)} × ${Math.round(r.height * dpr)}`;
  // 选区贴近屏幕顶部时尺寸标签放进框内，避免超出屏幕
  sizeLabel.classList.toggle("inside", r.top < 28);
}

overlay.addEventListener("pointerdown", (e) => {
  if (e.button === 2) {
    cancel();
    return;
  }
  if (e.button !== 0) return;
  dragging = true;
  startX = e.clientX;
  startY = e.clientY;
  overlay.setPointerCapture(e.pointerId);
  overlay.classList.add("selecting");
  hint.classList.add("hidden");
  box.classList.remove("hidden");
  updateBox(e.clientX, e.clientY);
});

overlay.addEventListener("pointermove", (e) => {
  if (!dragging) return;
  updateBox(e.clientX, e.clientY);
});

overlay.addEventListener("pointerup", (e) => {
  if (!dragging || e.button !== 0) return;
  dragging = false;
  const r = rectFrom(e.clientX, e.clientY);
  const dpr = window.devicePixelRatio || 1;
  reset();

  // 过小视为误触，仅取消
  if (r.width < 4 || r.height < 4) {
    void getCurrentWindow().hide();
    return;
  }
  void invoke("snip_capture", {
    x: r.left * dpr,
    y: r.top * dpr,
    w: r.width * dpr,
    h: r.height * dpr,
  });
});

overlay.addEventListener("contextmenu", (e) => e.preventDefault());

window.addEventListener("keydown", (e) => {
  if (e.key === "Escape") cancel();
});

// 每次呼出时重置上一次的选区状态
void listen("snip-start", reset);
