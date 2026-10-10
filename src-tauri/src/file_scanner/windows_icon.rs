use std::{mem::size_of, os::windows::ffi::OsStrExt, path::Path, ptr::null_mut};

use base64::{engine::general_purpose::STANDARD, Engine};
use windows_sys::Win32::{
    Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GdiFlush, SelectObject,
        BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ,
    },
    UI::{
        Shell::SHDefExtractIconW,
        WindowsAndMessaging::{DestroyIcon, DrawIconEx, DI_NORMAL, HICON},
    },
};

const SIZE: usize = 96;
const BYTE_LENGTH: usize = SIZE * SIZE * 4;

#[derive(Default)]
struct IconResources {
    icon: HICON,
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
}

impl Drop for IconResources {
    fn drop(&mut self) {
        unsafe {
            if !self.previous.is_null() {
                SelectObject(self.dc, self.previous);
            }
            if !self.bitmap.is_null() {
                DeleteObject(self.bitmap);
            }
            if !self.dc.is_null() {
                DeleteDC(self.dc);
            }
            if !self.icon.is_null() {
                DestroyIcon(self.icon);
            }
        }
    }
}

fn win32_error(operation: &str) -> String {
    format!("{operation}: {}", std::io::Error::last_os_error())
}

pub(super) fn load(source: &str) -> Result<String, String> {
    let path = Path::new(source);
    if source.contains('\0') || !path.is_absolute() {
        return Err("Icon source must be an absolute EXE path without NUL characters".into());
    }
    if !path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
    {
        return Err("Icon source must be an EXE file".into());
    }
    let metadata = path
        .metadata()
        .map_err(|error| format!("Cannot open icon source: {error}"))?;
    if !metadata.is_file() {
        return Err("Icon source must point to a file".into());
    }
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut resources = IconResources::default();
    let mut bits = null_mut();
    // 所有 GDI 句柄及 DIB 内存仅在当前后台线程使用，由 resources 统一释放。
    let (black, white) = unsafe {
        let result = SHDefExtractIconW(
            path.as_ptr(),
            0,
            0,
            &mut resources.icon,
            null_mut(),
            SIZE as u32,
        );
        if result != 0 || resources.icon.is_null() {
            return Err(format!(
                "Cannot extract EXE icon (HRESULT 0x{:08X}); the file may not contain an icon",
                result as u32
            ));
        }
        resources.dc = CreateCompatibleDC(null_mut());
        if resources.dc.is_null() {
            return Err(win32_error("CreateCompatibleDC failed"));
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: SIZE as i32,
                biHeight: -(SIZE as i32), // 自顶向下，避免 PNG 上下翻转。
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..Default::default()
            },
            ..Default::default()
        };
        resources.bitmap = CreateDIBSection(
            resources.dc,
            &info,
            DIB_RGB_COLORS,
            &mut bits,
            null_mut(),
            0,
        );
        if resources.bitmap.is_null() || bits.is_null() {
            return Err(win32_error("CreateDIBSection failed"));
        }
        let previous = SelectObject(resources.dc, resources.bitmap);
        if previous.is_null() || previous as isize == -1 {
            return Err(win32_error("SelectObject failed"));
        }
        resources.previous = previous;
        let render = |background: u8| -> Result<Vec<u8>, String> {
            std::ptr::write_bytes(bits.cast::<u8>(), background, BYTE_LENGTH);
            if DrawIconEx(
                resources.dc,
                0,
                0,
                resources.icon,
                SIZE as i32,
                SIZE as i32,
                0,
                null_mut(),
                DI_NORMAL,
            ) == 0
            {
                GdiFlush();
                return Err(win32_error("DrawIconEx failed"));
            }
            if GdiFlush() == 0 {
                return Err(win32_error("GdiFlush failed"));
            }
            Ok(std::slice::from_raw_parts(bits.cast::<u8>(), BYTE_LENGTH).to_vec())
        };
        (render(0)?, render(255)?)
    };
    // 黑白背景差值恢复透明度，同时兼容带 alpha 和传统掩码图标。
    // GDI 输出为预乘 BGRA，PNG 使用非预乘 RGBA。
    let mut rgba = Vec::with_capacity(BYTE_LENGTH);
    for (black, white) in black.chunks_exact(4).zip(white.chunks_exact(4)) {
        let transparency = (0..3)
            .map(|channel| white[channel].saturating_sub(black[channel]))
            .max()
            .unwrap_or(0);
        let alpha = 255 - transparency;
        for channel in [2, 1, 0] {
            rgba.push(if alpha == 0 {
                0
            } else {
                ((u32::from(black[channel]) * 255 + u32::from(alpha) / 2) / u32::from(alpha))
                    .min(255) as u8
            });
        }
        rgba.push(alpha);
    }
    let mut png_bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png_bytes, SIZE as u32, SIZE as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|error| format!("Cannot encode icon: {error}"))?;
        writer
            .write_image_data(&rgba)
            .map_err(|error| format!("Cannot encode icon: {error}"))?;
        writer
            .finish()
            .map_err(|error| format!("Cannot finish icon PNG: {error}"))?;
    }
    Ok(format!(
        "data:image/png;base64,{}",
        STANDARD.encode(png_bytes)
    ))
}
