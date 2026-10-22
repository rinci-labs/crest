//! File thumbnails for the shelf, taken from Windows (the same ones File Explorer shows).
//! Done on a separate thread because video/PDF thumbnails can take a while.

use std::path::{Path, PathBuf};
use std::thread;

use windows::Win32::Foundation::{E_FAIL, SIZE};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC,
    DeleteObject, GetDIBits, GetObjectW, HBITMAP,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize, IBindCtx};
use windows::Win32::UI::Shell::{IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_RESIZETOFIT};
use windows::core::{HSTRING, Result};

use crate::system::services::{Event, Notifier};

/// Longest thumbnail side in pixels; sharp for 48 DIP up to 200% scale.
const THUMB_PX: i32 = 96;

pub struct Thumbnail {
    pub id: u64,
    pub width: u32,
    pub height: u32,
    /// BGRA premultiplied.
    pub pixels: Vec<u8>,
}

pub fn spawn(items: Vec<(u64, PathBuf)>, notify: Notifier) {
    if items.is_empty() {
        return;
    }
    let spawned = thread::Builder::new().name("thumbnail".into()).spawn(move || {
        let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        for (id, path) in items {
            match extract(&path) {
                Ok((width, height, pixels)) => notify.send(Event::Thumbnail(Thumbnail { id, width, height, pixels })),
                Err(e) => debug_log!("thumbnail {} failed: {e}", path.display()),
            }
        }
        unsafe { CoUninitialize() };
    });
    if let Err(e) = spawned {
        error_log!("failed to create thumbnail thread: {e}");
    }
}

fn extract(path: &Path) -> Result<(u32, u32, Vec<u8>)> {
    unsafe {
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(&HSTRING::from(path.as_os_str()), None::<&IBindCtx>)?;
        let bitmap = factory.GetImage(SIZE { cx: THUMB_PX, cy: THUMB_PX }, SIIGBF_RESIZETOFIT)?;
        let result = pixels(bitmap);
        let _ = DeleteObject(bitmap.into());
        result
    }
}

fn pixels(bitmap: HBITMAP) -> Result<(u32, u32, Vec<u8>)> {
    unsafe {
        let mut info = BITMAP::default();
        let size = size_of::<BITMAP>() as i32;
        if GetObjectW(bitmap.into(), size, Some((&mut info as *mut BITMAP).cast())) == 0 {
            return Err(E_FAIL.into());
        }
        let (width, height) = (info.bmWidth, info.bmHeight.abs());
        let mut header = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut px = vec![0u8; (width * height * 4) as usize];
        let dc = CreateCompatibleDC(None);
        let lines = GetDIBits(dc, bitmap, 0, height as u32, Some(px.as_mut_ptr().cast()), &mut header, DIB_RGB_COLORS);
        let _ = DeleteDC(dc);
        if lines == 0 {
            return Err(E_FAIL.into());
        }
        fix_alpha(&mut px);
        Ok((width as u32, height as u32, px))
    }
}

/// Photos usually have no alpha (all 0) → make them opaque. Icons sometimes have straight
/// alpha → convert to premultiplied as Direct2D expects.
fn fix_alpha(px: &mut [u8]) {
    let (px, _) = px.as_chunks_mut::<4>();
    if px.iter().all(|p| p[3] == 0) {
        px.iter_mut().for_each(|p| p[3] = 255);
    } else if px.iter().any(|p| p[0] > p[3] || p[1] > p[3] || p[2] > p[3]) {
        for p in px.iter_mut() {
            let a = p[3] as u32;
            for c in &mut p[..3] {
                *c = (*c as u32 * a / 255) as u8;
            }
        }
    }
}
