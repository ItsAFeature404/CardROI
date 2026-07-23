//! Attaching a photo of a physical card to a holding. `cardroi-web` has
//! no filesystem, so every upload goes through `PhotoStorage::Inline`
//! (full-size bytes stored directly in the row) rather than the
//! disk-backed path native/CLI uses - see
//! `cardroi::db::repository::PhotoStorage`.
//!
//! Getting bytes off the browser's file picker needs no raw `web-sys`
//! wiring: Dioxus 0.7's own `FormData::files()`/`FileData::read_bytes()`
//! already does the real `FileReader` round-trip internally on the web
//! target. `accept="image/*" capture="environment"` is enough to open a
//! phone's camera directly on mobile browsers, falling back to a plain
//! file picker on desktop - `capture`'s exact behavior varies by mobile
//! browser and can only be confirmed by hand on a real phone, not by any
//! automated test.

use cardroi::db::repository::PhotoStorage;
use cardroi::models::HoldingImage;
use dioxus::prelude::*;

use crate::web_bridge::WebBridge;

/// A hidden file input behind a plain text/icon trigger - `uploading`
/// disables it and swaps the label so a second tap mid-upload can't fire
/// a second overlapping request.
///
/// `current_photo_id`: v1 ships single-primary-photo only, so uploading
/// a second photo replaces the first rather than adding to it - deleted
/// before the new upload, not after, so a failed upload never leaves the
/// holding with zero photos it didn't ask to lose. Without this, the
/// repository's own "first upload is primary" rule would make a second
/// upload silently become a non-primary photo this UI never shows,
/// which would look like the upload did nothing.
#[component]
pub fn PhotoCapture(
    holding_id: i64,
    current_photo_id: Option<i64>,
    on_uploaded: EventHandler<HoldingImage>,
) -> Element {
    let bridge = use_context::<WebBridge>();
    let mut uploading = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);

    let onchange = move |evt: FormEvent| {
        let bridge = bridge.clone();
        let Some(file) = evt.files().into_iter().next() else {
            return;
        };
        // Set before the first await, not after - lets the render loop
        // actually paint "Uploading..." before the CPU-heavy decode/
        // resize/encode work (inside bridge.run below, which has no
        // yield point of its own) blocks the single UI thread.
        uploading.set(true);
        error.set(None);
        spawn(async move {
            match file.read_bytes().await {
                Ok(bytes) => {
                    let bytes = bytes.to_vec();
                    let outcome = bridge
                        .run(move |repo| {
                            if let Some(existing_id) = current_photo_id {
                                repo.delete_photo(existing_id, PhotoStorage::Inline)?;
                            }
                            repo.add_photo(holding_id, &bytes, PhotoStorage::Inline)
                        })
                        .await;
                    uploading.set(false);
                    match outcome {
                        Ok(photo) => on_uploaded.call(photo),
                        Err(err) => error.set(Some(err.to_string())),
                    }
                }
                Err(err) => {
                    uploading.set(false);
                    error.set(Some(err.to_string()));
                }
            }
        });
    };

    rsx! {
        div { class: "flex flex-col gap-2",
            label { class: "text-gold text-sm cursor-pointer",
                if uploading() {
                    "Uploading..."
                } else if current_photo_id.is_some() {
                    "Replace photo"
                } else {
                    "Add a photo"
                }
                input {
                    r#type: "file",
                    accept: "image/*",
                    capture: "environment",
                    class: "hidden",
                    disabled: uploading(),
                    onchange,
                }
            }
            if let Some(err) = error() {
                p { class: "text-loss text-sm m-0", "{err}" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use cardroi::db::open_in_memory;
    use cardroi::db::repository::Repository;
    use cardroi::models::{NewCard, NewHolding, NewSet};
    use wasm_bindgen_test::wasm_bindgen_test;

    use super::*;

    // This is the first time the `image` crate's decode/resize/JPEG-
    // encode path (used by `Repository::add_photo`) actually *executes*
    // on wasm32, not just compiles - nothing in the root crate's own
    // (native-only) test suite proves that. `PhotoCapture` itself needs
    // real browser File/FileReader APIs this project's Node-based wasm
    // test runner doesn't provide (see CLAUDE.md), so that half can only
    // be checked by hand in a real browser - this test covers the half
    // that's actually about this data model, not the DOM.

    fn seed_holding() -> (Repository, i64) {
        let repo = Repository::new(open_in_memory().unwrap());
        let set = repo
            .create_set(&NewSet {
                name: "Test Set".to_string(),
                sport: "Basketball".to_string(),
                ..Default::default()
            })
            .unwrap();
        let card = repo
            .create_card(&NewCard {
                set_id: set.id,
                card_number: "1".to_string(),
                player_name: "Test Player".to_string(),
                ..Default::default()
            })
            .unwrap();
        let holding_id = repo
            .create_holding(&NewHolding {
                card_id: card.id,
                ..Default::default()
            })
            .unwrap()
            .id;
        (repo, holding_id)
    }

    /// A real, decodable JPEG, solid-colored - enough to exercise
    /// decode/resize/re-encode without a checked-in binary fixture, same
    /// technique `tests/holding_images.rs` uses natively.
    fn fake_jpeg(width: u32, height: u32) -> Vec<u8> {
        let img = image::ImageBuffer::from_pixel(width, height, image::Rgb([200u8, 50, 50]));
        let mut bytes = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 90)
            .encode(&img, width, height, image::ExtendedColorType::Rgb8)
            .unwrap();
        bytes
    }

    #[wasm_bindgen_test]
    fn add_photo_inline_decodes_resizes_and_stores_full_bytes_in_the_row() {
        let (repo, holding_id) = seed_holding();

        let photo = repo
            .add_photo(holding_id, &fake_jpeg(3000, 1500), PhotoStorage::Inline)
            .unwrap();

        // Capped to the Inline budget (smaller than Disk's, see
        // db::repository::holding_images) - proof the resize step
        // actually ran on wasm32, not just compiled there.
        assert_eq!(photo.width, 1200);
        assert_eq!(photo.height, 600);
        assert!(photo.file_path.is_none());
        let full_data = photo
            .full_data
            .expect("inline storage must populate full_data");
        assert!(!full_data.is_empty());
        assert!(!photo.thumbnail_data.is_empty());
    }
}
