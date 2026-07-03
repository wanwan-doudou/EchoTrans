# EchoTrans

在任意应用中选中文本，**连按三次 Ctrl+C**，译文立即在鼠标旁弹出。

机器翻译与 AI 翻译**双通道并行**：微软翻译几百毫秒先出结果，AI 译文流式跟进，两相对照。AI 通道兼容一切 OpenAI Chat Completions 格式的接口（OpenAI、DeepSeek、Kimi、通义千问、智谱、Ollama 等）。

基于 Tauri 2 + 原生 TypeScript，安装包仅 ~3MB。

## 功能特性

- **三连 Ctrl+C 触发**：被动监听键盘（不注册全局快捷键、不拦截按键），系统复制功能完全不受影响
- **双通道对照**：微软免费机器翻译（免注册免配置）+ AI 流式翻译同时进行，互不阻塞
- **鼠标旁悬浮窗**：无边框深色卡片跟随鼠标弹出，多显示器边缘自适应；Esc、点击外部或 ✕ 关闭
- **中英自动互译**：原文以中文为主译为英文，否则译为简体中文；方向与风格可通过系统提示词自定义
- **词汇学习友好**：单个单词会给出词性与常用义项，而非原样返回
- **常驻托盘**：左键打开设置，右键菜单退出；关闭窗口只是隐藏，单实例防多开
- **隐私**：无中间服务器，文本直连微软接口与你自己配置的 AI 接口；配置仅保存在本地

## 安装

从 [Releases](../../releases) 下载 `EchoTrans_x.x.x_x64-setup.exe` 安装，或参考下方自行构建。

首次启动会自动弹出设置页，填入接口地址、API Key、模型名，点「保存并测试」验证连通即可使用。

## 使用

1. 在任意应用中选中文本
2. 连按三次 `Ctrl+C`（600ms 间隔内）
3. 悬浮窗弹出：上半区微软翻译先出，下半区 AI 译文流式输出
4. 按 `Esc` 或点击窗口外部收起

## 配置说明

| 设置项 | 说明 |
| --- | --- |
| 接口地址 | OpenAI 兼容的 Base URL，如 `https://api.openai.com/v1`、`https://api.deepseek.com/v1` |
| API Key | 对应服务的密钥，仅存本地 |
| 模型 | 如 `gpt-4o-mini`、`deepseek-chat`、`qwen-turbo` |
| 系统提示词 | 决定翻译方向与风格，可一键恢复默认 |
| 温度 | 默认 0.3；部分推理模型只接受 1 |
| 微软机器翻译开关 | 关闭后悬浮窗恢复纯 AI 单区域 |

配置文件位置：`%APPDATA%\com.echotrans.app\config.json`

## 开发

环境要求：Rust 1.77+、Node.js 18+、pnpm。

```bash
pnpm install
pnpm tauri dev     # 开发运行
pnpm tauri build   # 打包，产物在 src-tauri/target/release/bundle/
```

## 常见问题

**构建时报 `Are you sure you have RC.EXE in your $PATH`**

机器上混装多版本 Windows SDK 时，`embed-resource` 可能探测不到资源编译器。新建 `src-tauri/.cargo/config.toml`（此文件不入库，属机器本地配置）：

```toml
[env]
RC = 'C:\Program Files (x86)\Windows Kits\10\bin\<你的SDK版本>\x64\rc.exe'
```

**在某些窗口里三连 Ctrl+C 没反应**

以管理员权限运行的程序会屏蔽普通权限进程的键盘钩子（Windows 权限隔离）。需要时以管理员身份运行 EchoTrans。

**机翻区报错但 AI 正常**

微软翻译走的是 Edge 浏览器的非官方公开接口，无官方 SLA；失效只影响机翻区，AI 通道完全独立。可在设置中关闭机翻。

## 技术架构

```
src-tauri/src/
├── hotkey.rs      # rdev 被动键盘监听，三连 Ctrl+C 检测与触发调度
├── mt.rs          # 微软 Edge 翻译通道（JWT 缓存 + 方向判定）
├── translator.rs  # OpenAI 兼容流式翻译（SSE 逐行解析）
├── config.rs      # 配置读写
├── tray.rs        # 系统托盘
└── lib.rs         # 组装：窗口事件、命令注册

src/
├── popup.ts       # 悬浮窗：双区域渲染、流式追加
└── settings.ts    # 设置页
```

## 更换图标

准备 1024x1024 的 `app-icon.png` 放项目根目录，执行 `pnpm tauri icon`，全部尺寸（exe / 托盘 / 安装器）自动重新生成。
