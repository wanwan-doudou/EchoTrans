import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";

interface AppConfig {
  api_base: string;
  api_key: string;
  model: string;
  system_prompt: string;
  temperature: number;
  enable_machine: boolean;
  theme: string;
  snip_hotkey: string;
}

const $ = <T extends HTMLElement = HTMLElement>(id: string) =>
  document.getElementById(id) as T;

const apiBaseEl = $<HTMLInputElement>("api_base");
const apiKeyEl = $<HTMLInputElement>("api_key");
const modelEl = $<HTMLInputElement>("model");
const promptEl = $<HTMLTextAreaElement>("system_prompt");
const temperatureEl = $<HTMLInputElement>("temperature");
const enableMachineEl = $<HTMLInputElement>("enable_machine");
const snipHotkeyEl = $<HTMLInputElement>("snip_hotkey");
const toggleKeyBtn = $<HTMLButtonElement>("toggle_key");
const saveBtn = $<HTMLButtonElement>("save");
const testBtn = $<HTMLButtonElement>("test");
const testResultEl = $("test_result");
const toastEl = $("toast");

let toastTimer: ReturnType<typeof setTimeout> | undefined;

function applyTheme(theme: string) {
  document.documentElement.dataset.theme = theme || "light";
}

function selectedTheme(): string {
  const checked = document.querySelector<HTMLInputElement>('input[name="theme"]:checked');
  return checked?.value ?? "light";
}

// 切换选项时即时预览（保存后才持久并同步到悬浮窗）
for (const radio of document.querySelectorAll<HTMLInputElement>('input[name="theme"]')) {
  radio.addEventListener("change", () => applyTheme(selectedTheme()));
}

function showToast(message: string) {
  toastEl.textContent = message;
  toastEl.classList.remove("hidden");
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => toastEl.classList.add("hidden"), 1800);
}

function collectConfig(): AppConfig {
  // 温度留空或非法时回退默认 0.3，避免误存为 0
  const rawTemperature = temperatureEl.value.trim();
  const temperature = Number(rawTemperature);
  return {
    api_base: apiBaseEl.value.trim(),
    api_key: apiKeyEl.value.trim(),
    model: modelEl.value.trim(),
    system_prompt: promptEl.value.trim(),
    temperature:
      rawTemperature !== "" && Number.isFinite(temperature) ? temperature : 0.3,
    enable_machine: enableMachineEl.checked,
    theme: selectedTheme(),
    snip_hotkey: snipHotkeyEl.value.trim(),
  };
}

async function loadConfig() {
  const cfg = await invoke<AppConfig>("get_config");
  apiBaseEl.value = cfg.api_base;
  apiKeyEl.value = cfg.api_key;
  modelEl.value = cfg.model;
  promptEl.value = cfg.system_prompt;
  temperatureEl.value = String(cfg.temperature);
  enableMachineEl.checked = cfg.enable_machine;
  snipHotkeyEl.value = cfg.snip_hotkey;
  const themeRadio = document.querySelector<HTMLInputElement>(
    `input[name="theme"][value="${cfg.theme}"]`,
  );
  const selected =
    themeRadio ?? document.querySelector<HTMLInputElement>('input[name="theme"][value="light"]');
  selected!.checked = true;
  applyTheme(selected!.value);

  // 配置就位后才允许保存，防止加载失败时把空表单覆盖进配置文件
  saveBtn.disabled = false;
  testBtn.disabled = false;
}

async function saveConfig(): Promise<boolean> {
  try {
    await invoke("save_config", { config: collectConfig() });
    return true;
  } catch (error) {
    showToast(`保存失败：${error}`);
    return false;
  }
}

$<HTMLButtonElement>("reset_prompt").addEventListener("click", async () => {
  promptEl.value = await invoke<string>("get_default_prompt");
  showToast("已填入默认提示词，记得保存");
});

toggleKeyBtn.addEventListener("click", () => {
  const hidden = apiKeyEl.type === "password";
  apiKeyEl.type = hidden ? "text" : "password";
  toggleKeyBtn.textContent = hidden ? "隐藏" : "显示";
});

saveBtn.addEventListener("click", async () => {
  if (await saveConfig()) {
    showToast("已保存 ✓");
  }
});

testBtn.addEventListener("click", async () => {
  if (!(await saveConfig())) return;

  testBtn.disabled = true;
  testBtn.textContent = "测试中…";
  testResultEl.classList.remove("hidden", "ok", "err");
  testResultEl.textContent = "正在请求接口，请稍候…";

  try {
    const result = await invoke<string>("test_translate", {
      text: "Hello! This is a connectivity test.",
    });
    testResultEl.classList.add("ok");
    testResultEl.textContent = `连接成功，译文：${result}`;
  } catch (error) {
    testResultEl.classList.add("err");
    testResultEl.textContent = `测试失败：${error}`;
  } finally {
    testBtn.disabled = false;
    testBtn.textContent = "保存并测试";
  }
});

// ---- 检查更新 ----

const versionEl = $("app_version");
const checkUpdateBtn = $<HTMLButtonElement>("check_update");
const updateStatusEl = $("update_status");
const installUpdateBtn = $<HTMLButtonElement>("install_update");

let pendingUpdate: Update | null = null;

void getVersion().then((version) => {
  versionEl.textContent = version;
});

checkUpdateBtn.addEventListener("click", async () => {
  checkUpdateBtn.disabled = true;
  checkUpdateBtn.textContent = "检查中…";
  updateStatusEl.classList.remove("hidden", "ok", "err");
  updateStatusEl.textContent = "正在检查更新…";
  installUpdateBtn.classList.add("hidden");
  pendingUpdate = null;

  try {
    pendingUpdate = await check();
    if (pendingUpdate) {
      updateStatusEl.classList.add("ok");
      const notes = pendingUpdate.body ? `\n${pendingUpdate.body}` : "";
      updateStatusEl.textContent = `发现新版本 v${pendingUpdate.version}${notes}`;
      installUpdateBtn.classList.remove("hidden");
      installUpdateBtn.disabled = false;
    } else {
      updateStatusEl.classList.add("ok");
      updateStatusEl.textContent = `已是最新版本 v${await getVersion()}`;
    }
  } catch (error) {
    updateStatusEl.classList.add("err");
    updateStatusEl.textContent = `检查更新失败：${error}`;
  } finally {
    checkUpdateBtn.disabled = false;
    checkUpdateBtn.textContent = "检查更新";
  }
});

installUpdateBtn.addEventListener("click", async () => {
  if (!pendingUpdate) return;
  installUpdateBtn.disabled = true;

  let downloaded = 0;
  let total = 0;
  try {
    await pendingUpdate.downloadAndInstall((event) => {
      if (event.event === "Started") {
        total = event.data.contentLength ?? 0;
        updateStatusEl.textContent = "开始下载更新…";
      } else if (event.event === "Progress") {
        downloaded += event.data.chunkLength;
        const percent = total ? Math.round((downloaded / total) * 100) : 0;
        updateStatusEl.textContent = `下载中… ${percent}%`;
      } else if (event.event === "Finished") {
        updateStatusEl.textContent = "下载完成，正在安装…";
      }
    });
    updateStatusEl.textContent = "安装完成，即将重启应用…";
    await relaunch();
  } catch (error) {
    updateStatusEl.classList.add("err");
    updateStatusEl.textContent = `更新失败：${error}`;
    installUpdateBtn.disabled = false;
  }
});

void loadConfig().catch(() => {
  showToast("配置加载失败，请关闭后重新打开设置窗口");
});
