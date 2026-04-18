//! Platform-specific badge overlay and system clock detection.

// --- Windows system clock detection ------------------------------

#[cfg(target_os = "windows")]
#[link(name = "kernel32")]
extern "system" {
    fn GetLocaleInfoW(locale: u32, lctype: u32, lp_lc_data: *mut u16, cch_data: i32) -> i32;
}

/// Returns true when the Windows regional settings use a 24-hour clock.
///
/// Reads `LOCALE_ITIME` ("0" = 12-hour, "1" = 24-hour) via `GetLocaleInfoW`.
#[cfg(target_os = "windows")]
#[allow(unsafe_code, reason = "GetLocaleInfoW is a safe Windows API call wrapped with an unsafe extern block")]
fn system_uses_24h() -> Option<bool> {
    const LOCALE_USER_DEFAULT: u32 = 0x0400;
    const LOCALE_ITIME: u32 = 0x0019;
    let mut buf = [0u16; 4];
    let len = unsafe { GetLocaleInfoW(LOCALE_USER_DEFAULT, LOCALE_ITIME, buf.as_mut_ptr(), 4) };
    if len <= 0 {
        return None;
    }
    Some(
        buf[..(len as usize).saturating_sub(1)]
            .first()
            .copied()
            .map(|c| c != b'0' as u16)
            .unwrap_or(false),
    )
}

/// On non-Windows, `WebView` Intl resolution is reliable so we return `None`
/// and let the frontend probe it directly.
#[cfg(not(target_os = "windows"))]
fn system_uses_24h() -> Option<bool> {
    None
}

/// Returns the OS-detected clock format for the "auto" time setting.
///
/// On Windows, `WebView2` (Chromium) derives the hour cycle from the ICU/CLDR
/// language-tag default (e.g. `en-US` is always 12h) and ignores the Windows
/// Region time-format setting, so the backend must read it directly.
/// Returns `None` on non-Windows platforms where the `WebView` Intl API
/// already honours the system locale.
pub fn system_clock_format() -> Option<&'static str> {
    system_uses_24h().map(|h24| if h24 { "24h" } else { "12h" })
}

// --- Badge overlay icon (Windows) ---------------------------------

/// Render a small 16x16 RGBA image with a red circle and white digit(s).
///
/// Used on Windows where `set_badge_count` is unsupported and the overlay
/// icon API must be used instead.
#[cfg(target_os = "windows")]
fn render_badge_icon(count: u32) -> Vec<u8> {
    const SIZE: usize = 16;
    let mut rgba = vec![0u8; SIZE * SIZE * 4];

    let cx = 7.5_f64;
    let cy = 7.5_f64;
    let r = 7.5_f64;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f64 - cx;
            let dy = y as f64 - cy;
            if dx * dx + dy * dy <= r * r {
                let i = (y * SIZE + x) * 4;
                rgba[i] = 220;     // R
                rgba[i + 1] = 38;  // G
                rgba[i + 2] = 38;  // B
                rgba[i + 3] = 255; // A
            }
        }
    }

    let label = if count > 99 {
        "!".to_string()
    } else {
        count.to_string()
    };
    stamp_text(&mut rgba, SIZE, &label);
    rgba
}

/// Tiny 3x5 pixel font for digits 0-9 and "!".
/// Each glyph is stored as 5 rows of 3 bits (MSB = left pixel).
#[cfg(target_os = "windows")]
fn glyph(ch: char) -> [u8; 5] {
    match ch {
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b010, 0b010, 0b010],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        '!' => [0b010, 0b010, 0b010, 0b000, 0b010],
        _   => [0b000; 5],
    }
}

/// Stamp a short text string (1-2 chars) centered in a 16x16 RGBA buffer.
#[cfg(target_os = "windows")]
fn stamp_text(rgba: &mut [u8], size: usize, text: &str) {
    let chars: Vec<char> = text.chars().collect();
    let glyph_w = 3;
    let glyph_h = 5;
    let spacing = 1;
    let total_w = chars.len() * glyph_w + chars.len().saturating_sub(1) * spacing;
    let start_x = (size.saturating_sub(total_w)) / 2;
    let start_y = (size.saturating_sub(glyph_h)) / 2;

    for (ci, &ch) in chars.iter().enumerate() {
        let g = glyph(ch);
        let ox = start_x + ci * (glyph_w + spacing);
        for (row, &bits) in g.iter().enumerate() {
            for col in 0..glyph_w {
                if bits & (1 << (glyph_w - 1 - col)) != 0 {
                    set_pixel(rgba, size, ox + col, start_y + row);
                }
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn set_pixel(rgba: &mut [u8], size: usize, px: usize, py: usize) {
    if px < size && py < size {
        let i = (py * size + px) * 4;
        rgba[i] = 255;
        rgba[i + 1] = 255;
        rgba[i + 2] = 255;
        rgba[i + 3] = 255;
    }
}

// --- Platform badge dispatch --------------------------------------

/// Windows implementation - overlay icon.
#[cfg(target_os = "windows")]
pub fn set_badge(window: &tauri::Window, count: Option<u32>) -> Result<(), String> {
    match count.filter(|&c| c > 0) {
        Some(c) => {
            let rgba = render_badge_icon(c);
            let image = tauri::image::Image::new_owned(rgba, 16, 16);
            window.set_overlay_icon(Some(image)).map_err(|e| e.to_string())
        }
        None => window.set_overlay_icon(None).map_err(|e| e.to_string()),
    }
}

/// Linux/macOS implementation - native badge count.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub fn set_badge(window: &tauri::Window, count: Option<u32>) -> Result<(), String> {
    let badge = count.filter(|&c| c > 0).map(i64::from);
    window.set_badge_count(badge).map_err(|e| e.to_string())
}

/// Android/iOS - badge counts are not supported, no-op.
#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn set_badge(_window: &tauri::Window, _count: Option<u32>) -> Result<(), String> {
    Ok(())
}
