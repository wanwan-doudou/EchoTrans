import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

interface StartPayload {
  text: string;
  model: string;
  machine: boolean;
}

function applyTheme(theme: string) {
  document.documentElement.dataset.theme = theme || "light";
}

// 启动时读取一次，之后设置页保存会广播 theme-changed
void invoke<{ theme: string }>("get_config").then((cfg) => applyTheme(cfg.theme));
void listen<string>("theme-changed", (event) => applyTheme(event.payload));

const $ = <T extends HTMLElement = HTMLElement>(id: string) =>
  document.getElementById(id) as T;

const statusEl = $("status");
const sourceEl = $("source");
const closeBtn = $<HTMLButtonElement>("close");

const mtSection = $("mt_section");
const mtOutputEl = $("mt_output");
const mtCopyBtn = $<HTMLButtonElement>("mt_copy");

const aiTagEl = $("ai_tag");
const aiOutputEl = $("ai_output");
const aiTextEl = $("ai_text");
const caretEl = $("caret");
const aiCopyBtn = $<HTMLButtonElement>("ai_copy");

let mtText = "";
let aiText = "";

type Status = "loading" | "done" | "error";

function setStatus(status: Status) {
  statusEl.className = `dot ${status}`;
  caretEl.classList.toggle("hidden", status !== "loading");
  aiOutputEl.classList.toggle("error", status === "error");
}

function bindCopy(btn: HTMLButtonElement, getText: () => string) {
  btn.addEventListener("click", async () => {
    const text = getText();
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
      btn.textContent = "已复制 ✓";
      setTimeout(() => (btn.textContent = "复制"), 1200);
    } catch {
      // 剪贴板暂不可用时静默失败
    }
  });
}

void listen<StartPayload>("translate-start", (event) => {
  const { text, model, machine } = event.payload;

  sourceEl.textContent = text.replace(/\s+/g, " ");
  sourceEl.title = text;

  mtSection.classList.toggle("hidden", !machine);
  mtText = "";
  mtOutputEl.textContent = machine ? "翻译中…" : "";
  mtOutputEl.classList.remove("error");
  mtOutputEl.classList.add("pending");
  mtCopyBtn.disabled = true;
  mtCopyBtn.textContent = "复制";

  aiText = "";
  aiTextEl.textContent = "";
  aiTagEl.textContent = `AI · ${model}`;
  aiCopyBtn.disabled = true;
  aiCopyBtn.textContent = "复制";
  setStatus("loading");
});

void listen<string>("mt-result", (event) => {
  mtText = event.payload;
  mtOutputEl.textContent = mtText;
  mtOutputEl.classList.remove("pending", "error");
  mtCopyBtn.disabled = false;
});

void listen<string>("mt-error", (event) => {
  mtOutputEl.textContent = event.payload;
  mtOutputEl.classList.remove("pending");
  mtOutputEl.classList.add("error");
});

void listen<string>("translate-chunk", (event) => {
  aiText += aiText ? event.payload : event.payload.trimStart();
  aiTextEl.textContent = aiText;
  aiOutputEl.scrollTop = aiOutputEl.scrollHeight;
});

void listen<string>("translate-done", (event) => {
  aiText = event.payload.trim();
  aiTextEl.textContent = aiText;
  aiCopyBtn.disabled = false;
  setStatus("done");
  aiOutputEl.scrollTop = aiOutputEl.scrollHeight;
});

void listen<string>("translate-error", (event) => {
  aiTextEl.textContent = event.payload;
  setStatus("error");
});

bindCopy(mtCopyBtn, () => mtText);
bindCopy(aiCopyBtn, () => aiText);

closeBtn.addEventListener("click", () => {
  void getCurrentWindow().hide();
});

window.addEventListener("keydown", (event) => {
  if (event.key === "Escape") {
    void getCurrentWindow().hide();
  }
});
