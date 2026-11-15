//! Windows clipboard through the Win32 clipboard API, read only (plus the snapshot / restore
//! that [`crate::selection::text_via_copy`] uses to put the user's clipboard back).
// Snapshot / restore is only used with `selection`.
#![cfg_attr(not(feature = "selection"), allow(dead_code, unused_imports))]

use crate::clipboard::{Image, Kind};
use crate::{Error, Result};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
use windows_sys::Win32::System::DataExchange::*;
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use windows_sys::Win32::System::Ole::{CF_BITMAP, CF_DIB, CF_DIBV5, CF_DSPBITMAP, CF_DSPENHMETAFILE, CF_DSPMETAFILEPICT, CF_ENHMETAFILE, CF_HDROP, CF_METAFILEPICT, CF_PALETTE, CF_UNICODETEXT};
use windows_sys::Win32::UI::Shell::{DragQueryFileW, HDROP};

/// Holds the clipboard open; closes it when dropped. Another app may hold it for a moment, so
/// opening retries for up to 200 ms.
struct Open;

impl Open {
    fn new() -> Result<Open> {
        let deadline = Instant::now() + Duration::from_millis(200);
        loop {
            if unsafe { OpenClipboard(std::ptr::null_mut()) } != 0 {
                return Ok(Open);
            }
            if Instant::now() > deadline {
                return Err(Error::msg("Another app is holding the clipboard"));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        unsafe { CloseClipboard() };
    }
}

fn available(f: u32) -> bool {
    unsafe { IsClipboardFormatAvailable(f) != 0 }
}

fn png_format() -> u32 {
    let name: Vec<u16> = "PNG\0".encode_utf16().collect();
    unsafe { RegisterClipboardFormatW(name.as_ptr()) }
}

fn html_format() -> u32 {
    let name: Vec<u16> = "HTML Format\0".encode_utf16().collect();
    unsafe { RegisterClipboardFormatW(name.as_ptr()) }
}

/// The bytes of a memory-backed format.
fn global_bytes(h: HANDLE) -> Option<Vec<u8>> {
    if h.is_null() {
        return None;
    }
    let size = unsafe { GlobalSize(h as HGLOBAL) };
    let p = unsafe { GlobalLock(h as HGLOBAL) } as *const u8;
    if p.is_null() {
        return None;
    }
    let v = unsafe { std::slice::from_raw_parts(p, size) }.to_vec();
    unsafe { GlobalUnlock(h as HGLOBAL) };
    Some(v)
}

fn data(f: u32) -> Option<Vec<u8>> {
    available(f).then(|| global_bytes(unsafe { GetClipboardData(f) })).flatten()
}

pub fn types() -> Result<Vec<Kind>> {
    let mut k = vec![];
    if available(CF_HDROP as u32) {
        k.push(Kind::Files);
    }
    if available(CF_UNICODETEXT as u32) {
        k.push(Kind::Text);
    }
    if available(png_format()) || available(CF_DIBV5 as u32) || available(CF_DIB as u32) {
        k.push(Kind::Image);
    }
    if available(html_format()) {
        k.push(Kind::Html);
    }
    Ok(k)
}

pub fn text() -> Result<Option<String>> {
    let _open = Open::new()?;
    Ok(data(CF_UNICODETEXT as u32).map(|b| {
        let units: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|u| *u != 0).collect();
        String::from_utf16_lossy(&units)
    }))
}

pub fn files() -> Result<Vec<PathBuf>> {
    let _open = Open::new()?;
    if !available(CF_HDROP as u32) {
        return Ok(vec![]);
    }
    let drop = unsafe { GetClipboardData(CF_HDROP as u32) } as HDROP;
    if drop.is_null() {
        return Ok(vec![]);
    }
    let n = unsafe { DragQueryFileW(drop, u32::MAX, std::ptr::null_mut(), 0) };
    let mut out = vec![];
    for i in 0..n.min(crate::clipboard::MAX_FILES as u32) {
        let len = unsafe { DragQueryFileW(drop, i, std::ptr::null_mut(), 0) } as usize;
        let mut buf = vec![0u16; len + 1];
        let got = unsafe { DragQueryFileW(drop, i, buf.as_mut_ptr(), buf.len() as u32) } as usize;
        out.push(PathBuf::from(String::from_utf16_lossy(&buf[..got])));
    }
    Ok(out)
}

/// A device-independent bitmap as a `.bmp` file: a 14-byte file header in front.
fn dib_to_bmp(dib: &[u8]) -> Option<Vec<u8>> {
    if dib.len() < 40 {
        return None;
    }
    let u32_at = |o: usize| u32::from_le_bytes([dib[o], dib[o + 1], dib[o + 2], dib[o + 3]]);
    let header = u32_at(0) as usize;
    let bit_count = u16::from_le_bytes([dib[14], dib[15]]) as usize;
    let compression = u32_at(16);
    let clr_used = u32_at(32) as usize;
    let colors = if clr_used != 0 { clr_used } else if bit_count <= 8 { 1 << bit_count } else { 0 };
    // BI_BITFIELDS with a plain BITMAPINFOHEADER: three color masks follow it.
    let masks = if compression == 3 && header == 40 { 12 } else { 0 };
    let offset = 14 + header + masks + colors * 4;
    let mut bmp = Vec::with_capacity(14 + dib.len());
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&((14 + dib.len()) as u32).to_le_bytes());
    bmp.extend_from_slice(&[0, 0, 0, 0]);
    bmp.extend_from_slice(&(offset as u32).to_le_bytes());
    bmp.extend_from_slice(dib);
    Some(bmp)
}

pub fn image() -> Result<Option<Image>> {
    let _open = Open::new()?;
    if let Some(png) = data(png_format()) {
        return Ok(Some(Image { mime: "image/png".into(), bytes: png }));
    }
    for f in [CF_DIBV5 as u32, CF_DIB as u32] {
        if let Some(bmp) = data(f).and_then(|d| dib_to_bmp(&d)) {
            return Ok(Some(Image { mime: "image/bmp".into(), bytes: bmp }));
        }
    }
    Ok(None)
}

// ── Snapshot / restore, for text_via_copy ───────────────────────────────────

/// GDI-handle formats (not memory): skipped; Windows synthesizes most of them again.
fn handle_format(f: u32) -> bool {
    [CF_BITMAP, CF_METAFILEPICT, CF_PALETTE, CF_ENHMETAFILE, CF_DSPBITMAP, CF_DSPMETAFILEPICT, CF_DSPENHMETAFILE].iter().any(|g| *g as u32 == f)
}

fn snapshot() -> Result<Vec<(u32, Vec<u8>)>> {
    let _open = Open::new()?;
    let mut out = vec![];
    let mut f = 0;
    loop {
        f = unsafe { EnumClipboardFormats(f) };
        if f == 0 {
            break;
        }
        if !handle_format(f) {
            if let Some(b) = global_bytes(unsafe { GetClipboardData(f) }) {
                out.push((f, b));
            }
        }
    }
    Ok(out)
}

fn restore(saved: &[(u32, Vec<u8>)]) -> Result {
    let _open = Open::new()?;
    unsafe { EmptyClipboard() };
    for (f, bytes) in saved {
        let h = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)) };
        if h.is_null() {
            continue;
        }
        let p = unsafe { GlobalLock(h) } as *mut u8;
        if p.is_null() {
            unsafe { GlobalFree(h) };
            continue;
        }
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len()) };
        unsafe { GlobalUnlock(h) };
        // On success the clipboard owns the memory.
        if unsafe { SetClipboardData(*f, h as HANDLE) }.is_null() {
            unsafe { GlobalFree(h) };
        }
    }
    Ok(())
}

/// Send the copy keystroke, read the text it put on the clipboard, put the previous clipboard
/// back. `Ok(None)` when nothing was copied.
pub(crate) fn copy_and_restore(send_copy_keys: impl FnOnce() -> Result) -> Result<Option<String>> {
    let saved = snapshot()?;
    let before = unsafe { GetClipboardSequenceNumber() };
    send_copy_keys()?;
    let deadline = Instant::now() + Duration::from_millis(700);
    while unsafe { GetClipboardSequenceNumber() } == before && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    if unsafe { GetClipboardSequenceNumber() } == before {
        return Ok(None);
    }
    // The copying app may still be writing more formats.
    std::thread::sleep(Duration::from_millis(30));
    let text = text();
    restore(&saved)?;
    text
}

#[cfg(test)]
mod tests {
    #[test]
    fn bmp_header() {
        let mut dib = vec![0u8; 40 + 4];
        dib[0] = 40; // biSize
        dib[14] = 32; // biBitCount
        let bmp = super::dib_to_bmp(&dib).unwrap();
        assert_eq!(&bmp[..2], b"BM");
        assert_eq!(u32::from_le_bytes([bmp[10], bmp[11], bmp[12], bmp[13]]), 54);
    }
}
