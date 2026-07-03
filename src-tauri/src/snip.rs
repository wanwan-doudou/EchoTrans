//! 截屏翻译：热键呼出选区遮罩 → 截取所选区域 → Windows 系统 OCR → 进入翻译管线。
//! OCR 使用系统内置的 WinRT 引擎，离线免费，无需任何配置。

use std::{sync::Mutex, time::Duration};

use tauri::{AppHandle, Emitter, Manager};
use windows::{
    core::HSTRING,
    Globalization::Language,
    Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap},
    Media::Ocr::OcrEngine,
    Storage::Streams::DataWriter,
    Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED},
};

use crate::hotkey;

/// 打开遮罩时记录目标显示器原点（全局物理坐标），截取时据此定位 xcap 显示器
pub struct SnipState(pub Mutex<Option<(i32, i32)>>);

struct RoApartment;

impl Drop for RoApartment {
    fn drop(&mut self) {
        unsafe {
            RoUninitialize();
        }
    }
}

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
            Ok(text) if !text.trim().is_empty() => {
                hotkey::translate_and_show(&app, text, true);
            }
            Ok(_) => hotkey::show_error_popup(
                &app,
                "屏幕截取",
                "未识别到文字，可尝试放大选区或确认文字清晰".into(),
            ),
            Err(msg) => hotkey::show_error_popup(&app, "屏幕截取", msg),
        }
    });
    Ok(())
}

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
    ocr_image(cropped)
}

fn ocr_image(img: image::RgbaImage) -> Result<String, String> {
    // blocking 线程可能被复用；仅在本次成功初始化时配对释放引用计数
    let _apartment = unsafe { RoInitialize(RO_INIT_MULTITHREADED) }
        .ok()
        .map(|_| RoApartment);

    let engine = OcrEngine::TryCreateFromUserProfileLanguages()
        .or_else(|_| {
            let lang = Language::CreateLanguage(&HSTRING::from("zh-Hans-CN"))?;
            OcrEngine::TryCreateFromLanguage(&lang)
        })
        .map_err(|_| "系统 OCR 引擎不可用，请在 Windows 设置中添加中文或英文语言包")?;

    // 小字号截图先放大，深色截图转为浅底深字，提高 WinRT OCR 的稳定性
    let max_dim = OcrEngine::MaxImageDimension().unwrap_or(2600);
    let img = prepare_for_ocr(img, max_dim);

    // RGBA -> BGRA（OCR 引擎接受的位图格式）
    let (w, h) = img.dimensions();
    let mut bgra = img.into_raw();
    for px in bgra.chunks_exact_mut(4) {
        px.swap(0, 2);
    }

    let ocr = || -> windows::core::Result<Vec<String>> {
        let writer = DataWriter::new()?;
        writer.WriteBytes(&bgra)?;
        let buffer = writer.DetachBuffer()?;
        let bitmap = SoftwareBitmap::CreateCopyFromBuffer(
            &buffer,
            BitmapPixelFormat::Bgra8,
            w as i32,
            h as i32,
        )?;
        let result = engine.RecognizeAsync(&bitmap)?.join()?;
        let mut lines = Vec::new();
        for line in result.Lines()? {
            lines.push(line.Text()?.to_string());
        }
        Ok(lines)
    };

    let lines = ocr().map_err(|e| format!("文字识别失败：{e}"))?;
    Ok(reflow_ocr_lines(&lines))
}

fn prepare_for_ocr(mut img: image::RgbaImage, max_dim: u32) -> image::RgbaImage {
    let sample_step = (img.width() as usize * img.height() as usize / 10_000).max(1);
    let mut luma_sum = 0u64;
    let mut sample_count = 0u64;
    for pixel in img.pixels().step_by(sample_step) {
        luma_sum += (299 * pixel[0] as u64 + 587 * pixel[1] as u64 + 114 * pixel[2] as u64) / 1000;
        sample_count += 1;
    }

    if sample_count > 0 && luma_sum / sample_count < 128 {
        for pixel in img.pixels_mut() {
            pixel[0] = 255 - pixel[0];
            pixel[1] = 255 - pixel[1];
            pixel[2] = 255 - pixel[2];
        }
    }

    let (w, h) = img.dimensions();
    let scale = (max_dim as f32 / w.max(h) as f32).min(2.0);
    if (scale - 1.0).abs() < 0.05 {
        return img;
    }

    let filter = if scale > 1.0 {
        image::imageops::FilterType::CatmullRom
    } else {
        image::imageops::FilterType::Triangle
    };
    image::imageops::resize(
        &img,
        ((w as f32 * scale) as u32).max(1),
        ((h as f32 * scale) as u32).max(1),
        filter,
    )
}

fn is_cjk(c: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&c)
        || ('\u{3000}'..='\u{303f}').contains(&c)
        || ('\u{ff00}'..='\u{ffef}').contains(&c)
}

/// WinRT OCR 的中文结果按"词"切分，词间带空格，拼回自然中文
fn clean_cjk_spaces(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (i, &c) in chars.iter().enumerate() {
        if c == ' ' && i > 0 && i + 1 < chars.len() && is_cjk(chars[i - 1]) && is_cjk(chars[i + 1])
        {
            continue;
        }
        out.push(c);
    }
    out
}

/// OCR 的逐行结果来自屏幕排版换行，不应直接作为翻译段落。
fn reflow_ocr_lines(lines: &[String]) -> String {
    let mut out = String::new();

    for line in lines {
        let line = clean_cjk_spaces(line.trim());
        if line.is_empty() {
            continue;
        }

        if !out.is_empty() {
            let previous = out.chars().next_back();
            let next = line.chars().next();
            let joins_directly = previous.is_some_and(|c| c == '-' || c == '‐' || c == '‑')
                || previous
                    .zip(next)
                    .is_some_and(|(a, b)| is_cjk(a) && is_cjk(b));
            if !joins_directly {
                out.push(' ');
            }
        }
        out.push_str(&line);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reflows_visual_line_breaks() {
        let lines = vec![
            "It came down dyed in neon magenta and sulfur-".into(),
            "yellow, coating the chrome chassis of passing".into(),
            "hover-trams in a poisonous shimmer.".into(),
        ];
        assert_eq!(
            reflow_ocr_lines(&lines),
            "It came down dyed in neon magenta and sulfur-yellow, coating the chrome chassis of passing hover-trams in a poisonous shimmer."
        );
    }

    #[test]
    fn inverts_dark_images_and_upscales_small_crops() {
        let img = image::RgbaImage::from_pixel(100, 50, image::Rgba([10, 20, 30, 255]));
        let prepared = prepare_for_ocr(img, 2600);
        assert_eq!(prepared.dimensions(), (200, 100));
        assert!(prepared.get_pixel(0, 0)[0] > 240);
        assert_eq!(prepared.get_pixel(0, 0)[3], 255);
    }
}
