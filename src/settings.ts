import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
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

function formatConfigError(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
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

function validateConfigInput(): string | null {
  if (!apiBaseEl.value.trim()) return "接口地址不能为空，已取消保存";
  if (!apiKeyEl.value.trim()) return "API Key 不能为空，已取消保存";
  if (!modelEl.value.trim()) return "模型不能为空，已取消保存";
  if (!promptEl.value.trim()) return "系统提示词不能为空，已取消保存";
  return null;
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
  const validationError = validateConfigInput();
  if (validationError) {
    showToast(validationError);
    return false;
  }

  try {
    await invoke("save_config", { config: collectConfig() });
    return true;
  } catch (error) {
    showToast(`保存失败：${formatConfigError(error)}`);
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
const manualDownloadBtn = $<HTMLButtonElement>("manual_download");

let pendingUpdate: Update | null = null;
let manualDownloadUrl = "";

const RELEASES_URL = "https://github.com/wanwan-doudou/EchoTrans/releases";
const CHECK_RETRY_DELAYS_MS = [800, 2_000, 5_000];
const DOWNLOAD_RETRY_DELAYS_MS = [1_000, 2_500, 5_000];
const CHECK_TIMEOUT_MS = 20_000;
const DOWNLOAD_TIMEOUT_MS = 180_000;

void getVersion().then((version) => {
  versionEl.textContent = version;
});

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function formatUpdateError(error: unknown): string {
  const message = error instanceof Error ? error.message : String(error);
  const normalized = message.replace(/\s+/g, " ").trim();
  return normalized.length > 280 ? `${normalized.slice(0, 280)}…` : normalized;
}

function isRetriableUpdateError(error: unknown): boolean {
  const message = formatUpdateError(error).toLowerCase();
  const nonRetriableTokens = [
    "signature",
    "checksum",
    "hash",
    "digest",
    "verify",
    "invalid json",
    "decoding response body",
  ];
  if (nonRetriableTokens.some((token) => message.includes(token))) {
    return false;
  }

  const retriableTokens = [
    "error sending request",
    "failed to fetch",
    "timeout",
    "timed out",
    "dns",
    "tls",
    "ssl",
    "proxy",
    "connection",
    "network",
  ];
  return retriableTokens.some((token) => message.includes(token));
}

async function retryWithBackoff<T>(
  action: (attempt: number) => Promise<T>,
  delays: readonly number[],
  onRetry: (attempt: number, delayMs: number, error: unknown) => void,
): Promise<T> {
  let lastError: unknown;
  for (let attempt = 1; attempt <= delays.length + 1; attempt += 1) {
    try {
      return await action(attempt);
    } catch (error) {
      lastError = error;
      const delayMs = delays[attempt - 1];
      if (!delayMs || !isRetriableUpdateError(error)) {
        throw error;
      }
      onRetry(attempt, delayMs, error);
      await sleep(delayMs);
    }
  }
  throw lastError;
}

function setUpdateStatus(kind: "info" | "ok" | "err", message: string) {
  updateStatusEl.classList.remove("hidden", "ok", "err");
  if (kind !== "info") {
    updateStatusEl.classList.add(kind);
  }
  updateStatusEl.textContent = message;
}

function extractUrl(value: unknown): string | null {
  if (typeof value === "string" && value.startsWith("http")) {
    return value;
  }
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return null;
  }

  const record = value as Record<string, unknown>;
  for (const key of ["url", "download_url", "html_url", "details_url"]) {
    const url = extractUrl(record[key]);
    if (url) return url;
  }
  return null;
}

function resolveManualDownloadUrl(update: Update): string {
  const platforms = update.rawJson.platforms;
  if (platforms && typeof platforms === "object" && !Array.isArray(platforms)) {
    const platformMap = platforms as Record<string, unknown>;
    const currentPlatformUrl = extractUrl(platformMap["windows-x86_64"]);
    if (currentPlatformUrl) return currentPlatformUrl;

    for (const platform of Object.values(platformMap)) {
      const platformUrl = extractUrl(platform);
      if (platformUrl) return platformUrl;
    }
  }

  const directUrl = extractUrl(update.rawJson);
  return directUrl ?? `${RELEASES_URL}/tag/v${update.version}`;
}

async function checkForUpdateWithRetry(): Promise<Update | null> {
  return retryWithBackoff(
    () => check({ timeout: CHECK_TIMEOUT_MS }),
    CHECK_RETRY_DELAYS_MS,
    (attempt, delayMs, error) => {
      setUpdateStatus(
        "info",
        `检查更新失败，${Math.round(delayMs / 1_000)} 秒后重试（${attempt}/${CHECK_RETRY_DELAYS_MS.length}）：${formatUpdateError(error)}`,
      );
    },
  );
}

async function downloadAndInstallWithRetry(): Promise<void> {
  await retryWithBackoff(
    async (attempt) => {
      const candidate = await check({ timeout: CHECK_TIMEOUT_MS });
      if (!candidate) {
        throw new Error("重新检查时未发现可安装的新版本");
      }

      pendingUpdate = candidate;
      manualDownloadUrl = resolveManualDownloadUrl(candidate);
      let downloaded = 0;
      let total = 0;
      const prefix =
        attempt > 1
          ? `第 ${attempt}/${DOWNLOAD_RETRY_DELAYS_MS.length + 1} 次下载：`
          : "";

      try {
        await candidate.downloadAndInstall(
          (event) => {
            if (event.event === "Started") {
              downloaded = 0;
              total = event.data.contentLength ?? 0;
              setUpdateStatus("info", `${prefix}开始下载更新…`);
            } else if (event.event === "Progress") {
              downloaded += event.data.chunkLength;
              const percent = total ? Math.round((downloaded / total) * 100) : 0;
              setUpdateStatus("info", `${prefix}下载中… ${percent}%`);
            } else if (event.event === "Finished") {
              setUpdateStatus("info", `${prefix}下载完成，正在安装…`);
            }
          },
          { timeout: DOWNLOAD_TIMEOUT_MS },
        );
      } catch (error) {
        await candidate.close().catch(() => undefined);
        throw error;
      }
    },
    DOWNLOAD_RETRY_DELAYS_MS,
    (attempt, delayMs, error) => {
      setUpdateStatus(
        "info",
        `下载更新失败，${Math.round(delayMs / 1_000)} 秒后重试（${attempt}/${DOWNLOAD_RETRY_DELAYS_MS.length}）：${formatUpdateError(error)}`,
      );
    },
  );
}

checkUpdateBtn.addEventListener("click", async () => {
  checkUpdateBtn.disabled = true;
  checkUpdateBtn.textContent = "检查中…";
  setUpdateStatus("info", "正在检查更新…");
  installUpdateBtn.classList.add("hidden");
  manualDownloadBtn.classList.add("hidden");
  pendingUpdate = null;
  manualDownloadUrl = "";

  try {
    pendingUpdate = await checkForUpdateWithRetry();
    if (pendingUpdate) {
      manualDownloadUrl = resolveManualDownloadUrl(pendingUpdate);
      const notes = pendingUpdate.body ? `\n${pendingUpdate.body}` : "";
      setUpdateStatus("ok", `发现新版本 v${pendingUpdate.version}${notes}`);
      installUpdateBtn.classList.remove("hidden");
      installUpdateBtn.disabled = false;
      manualDownloadBtn.classList.remove("hidden");
      manualDownloadBtn.disabled = false;
    } else {
      setUpdateStatus("ok", `已是最新版本 v${await getVersion()}`);
    }
  } catch (error) {
    console.error("检查更新失败", error);
    setUpdateStatus("err", `检查更新失败：${formatUpdateError(error)}`);
  } finally {
    checkUpdateBtn.disabled = false;
    checkUpdateBtn.textContent = "检查更新";
  }
});

installUpdateBtn.addEventListener("click", async () => {
  if (!pendingUpdate) return;
  installUpdateBtn.disabled = true;
  manualDownloadBtn.disabled = true;

  try {
    await downloadAndInstallWithRetry();
    setUpdateStatus("ok", "安装完成，即将重启应用…");
    await relaunch();
  } catch (error) {
    console.error("更新失败", error);
    const fallbackTip = manualDownloadUrl
      ? "\n可重试，或点击“浏览器下载更新”手动安装。"
      : "";
    setUpdateStatus("err", `更新失败：${formatUpdateError(error)}${fallbackTip}`);
    installUpdateBtn.disabled = false;
    manualDownloadBtn.disabled = false;
  }
});

manualDownloadBtn.addEventListener("click", async () => {
  const url = manualDownloadUrl || RELEASES_URL;
  manualDownloadBtn.disabled = true;

  try {
    await openUrl(url);
  } catch (error) {
    console.error("打开下载链接失败", error);
    setUpdateStatus(
      "err",
      `打开浏览器失败：${formatUpdateError(error)}\n下载地址：${url}`,
    );
  } finally {
    manualDownloadBtn.disabled = false;
  }
});

void loadConfig().catch((error) => {
  saveBtn.disabled = true;
  testBtn.disabled = true;
  testResultEl.classList.remove("hidden", "ok");
  testResultEl.classList.add("err");
  testResultEl.textContent = `配置加载失败：${formatConfigError(error)}\n为避免覆盖原配置，已禁用保存。请先重启应用；如果仍失败，检查配置文件或备份文件。`;
  showToast("配置加载失败，已禁用保存");
});
