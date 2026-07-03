//! 截屏翻译：热键呼出选区遮罩 → 截取所选区域 → 截图直接交给视觉模型识别并翻译。
//! 不做本地 OCR，识别质量取决于所配置模型的视觉能力。

use std::{io::Cursor, sync::Mutex, time::Duration};

use base64::Engine;
use tauri::{AppHandle, Emitter, Manager};

use crate::hotkey;

/// 视觉模型对超大图消耗 token 且识别无增益，超过此边长按比例缩小
const MAX_IMAGE_DIM: u32 = 2048;

/// 打开遮罩时记录目标显示器原点（全局物理坐标），截取时据此定位 xcap 显示器
pub struct SnipState(pub Mutex<Option<(i32, i32)>>);

pub fn open_overlay(app: &AppHandle) {
    let Some(win) = app.get_webview_window("snip") else {
        return;
    };
    // 按住快捷键会连续触发 KeyPress，遮罩已打开时忽略
    if win.is_visible().unwrap_or(false) {
        return;
    }
    let Ok(cursor) = app.cursor_position() else {
        return;
    };
    let Ok(Some(monitor)) = app.monitor_from_point(cursor.x, cursor.y) else {
        return;
    };

    let pos = *monitor.position();
    let size = *monitor.size();
    if let Ok(mut guard) = app.state::<SnipState>().0.lock() {
        *guard = Some((pos.x, pos.y));
    }

    let _ = win.set_position(pos);
    let _ = win.set_size(size);
    let _ = app.emit_to("snip", "snip-start", ());
    let _ = win.show();
    let _ = win.set_focus();
}

#[tauri::command]
pub async fn snip_capture(app: AppHandle, x: f64, y: f64, w: f64, h: f64) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("snip") {
        let _ = win.hide();
    }

    let region = (
        x.max(0.0) as u32,
        y.max(0.0) as u32,
        w.max(0.0) as u32,
        h.max(0.0) as u32,
    );
    // 误触的极小选区直接忽略
    if region.2 < 5 || region.3 < 5 {
        return Ok(());
    }

    let monitor_pos = app
        .state::<SnipState>()
        .0
        .lock()
        .ok()
        .and_then(|mut guard| guard.take())
        .ok_or("截屏上下文已失效，请重新呼出")?;

    tauri::async_runtime::spawn_blocking(move || {
        // 等遮罩窗口从屏幕上真正消失，避免把遮罩自己截进去
        std::thread::sleep(Duration::from_millis(160));
        match do_capture(region, monitor_pos) {
            Ok(png_base64) => hotkey::translate_image_and_show(&app, png_base64),
            Err(msg) => hotkey::show_error_popup(&app, "屏幕截取", msg),
        }
    });
    Ok(())
}

/// 截取选区并编码为 PNG base64
fn do_capture(region: (u32, u32, u32, u32), monitor_pos: (i32, i32)) -> Result<String, String> {
    let monitors = xcap::Monitor::all().map_err(|e| format!("枚举显示器失败：{e}"))?;
    let monitor = monitors
        .into_iter()
        .find(|m| m.x().ok() == Some(monitor_pos.0) && m.y().ok() == Some(monitor_pos.1))
        .ok_or("未找到目标显示器")?;

    let img = monitor
        .capture_image()
        .map_err(|e| format!("截屏失败：{e}"))?;

    let (iw, ih) = img.dimensions();
    let x = region.0.min(iw.saturating_sub(1));
    let y = region.1.min(ih.saturating_sub(1));
    let w = region.2.min(iw - x);
    let h = region.3.min(ih - y);
    if w < 5 || h < 5 {
        return Err("选区超出屏幕范围".into());
    }

    let cropped = image::imageops::crop_imm(&img, x, y, w, h).to_image();
    let (cw, ch) = cropped.dimensions();
    let cropped = if cw > MAX_IMAGE_DIM || ch > MAX_IMAGE_DIM {
        let scale = MAX_IMAGE_DIM as f32 / cw.max(ch) as f32;
        image::imageops::resize(
            &cropped,
            ((cw as f32 * scale) as u32).max(1),
            ((ch as f32 * scale) as u32).max(1),
            image::imageops::FilterType::Triangle,
        )
    } else {
        cropped
    };

    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(cropped)
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| format!("图片编码失败：{e}"))?;
    Ok(base64::engine::general_purpose::STANDARD.encode(&png))
}
