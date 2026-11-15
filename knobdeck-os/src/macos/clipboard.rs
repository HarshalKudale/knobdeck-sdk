//! macOS clipboard through `NSPasteboard`, read only (plus the snapshot / restore that
//! [`crate::selection::text_via_copy`] uses to put the user's clipboard back).

use crate::clipboard::{parse_uri_list, Image, Kind};
use crate::Result;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSPasteboard, NSPasteboardItem, NSPasteboardWriting};
use objc2_foundation::{NSArray, NSData, NSString};
use std::path::PathBuf;

const TEXT: &str = "public.utf8-plain-text";
const FILE_URL: &str = "public.file-url";
const PNG: &str = "public.png";
const TIFF: &str = "public.tiff";
const HTML: &str = "public.html";

fn board() -> Retained<NSPasteboard> {
    NSPasteboard::generalPasteboard()
}

fn type_names(pb: &NSPasteboard) -> Vec<String> {
    pb.types().map(|a| a.iter().map(|t| t.to_string()).collect()).unwrap_or_default()
}

pub fn types() -> Result<Vec<Kind>> {
    let names = type_names(&board());
    let has = |t: &str| names.iter().any(|n| n == t);
    let mut k = vec![];
    if has(FILE_URL) {
        k.push(Kind::Files);
    }
    if has(TEXT) || has("public.plain-text") {
        k.push(Kind::Text);
    }
    if has(PNG) || has(TIFF) || has("public.jpeg") {
        k.push(Kind::Image);
    }
    if has(HTML) {
        k.push(Kind::Html);
    }
    Ok(k)
}

pub fn text() -> Result<Option<String>> {
    let s = board().stringForType(&NSString::from_str(TEXT));
    Ok(s.map(|s| s.to_string()))
}

pub fn files() -> Result<Vec<PathBuf>> {
    let Some(items) = board().pasteboardItems() else { return Ok(vec![]) };
    let ty = NSString::from_str(FILE_URL);
    let uris: Vec<String> = items.iter().filter_map(|i| i.stringForType(&ty)).map(|s| s.to_string()).collect();
    Ok(parse_uri_list(&uris.join("\n")))
}

pub fn image() -> Result<Option<Image>> {
    let pb = board();
    for (ty, mime) in [(PNG, "image/png"), (TIFF, "image/tiff")] {
        if let Some(d) = pb.dataForType(&NSString::from_str(ty)) {
            return Ok(Some(Image { mime: mime.into(), bytes: d.to_vec() }));
        }
    }
    Ok(None)
}

#[cfg_attr(not(feature = "selection"), allow(dead_code))]
pub(crate) fn change_count() -> isize {
    board().changeCount()
}

/// Every item's data, per type.
#[cfg_attr(not(feature = "selection"), allow(dead_code))]
pub(crate) struct Snapshot(Vec<Vec<(Retained<NSString>, Retained<NSData>)>>);

#[cfg_attr(not(feature = "selection"), allow(dead_code))]
pub(crate) fn snapshot() -> Snapshot {
    let items = board().pasteboardItems();
    Snapshot(
        items
            .map(|items| {
                items
                    .iter()
                    .map(|item| {
                        let types = item.types();
                        types.iter().filter_map(|t| item.dataForType(&t).map(|d| (t.clone(), d))).collect()
                    })
                    .collect()
            })
            .unwrap_or_default(),
    )
}

#[cfg_attr(not(feature = "selection"), allow(dead_code))]
pub(crate) fn restore(s: &Snapshot) {
    let pb = board();
    pb.clearContents();
    if s.0.is_empty() {
        return;
    }
    let items: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> = s
        .0
        .iter()
        .map(|types| {
            let item = NSPasteboardItem::new();
            for (t, d) in types {
                item.setData_forType(d, t);
            }
            ProtocolObject::from_retained(item)
        })
        .collect();
    pb.writeObjects(&NSArray::from_retained_slice(&items));
}
