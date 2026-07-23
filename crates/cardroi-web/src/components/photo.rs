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

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use cardroi::db::repository::PhotoStorage;
use cardroi::models::HoldingImage;
use dioxus::prelude::*;
use dioxus_free_icons::Icon;
use dioxus_free_icons::icons::ld_icons::LdX;

use crate::web_bridge::WebBridge;

/// A hidden file input behind a plain text/icon trigger - `uploading`
/// disables it and swaps the label so a second tap mid-upload can't fire
/// a second overlapping request. Every upload appends a new photo -
/// `add_photo`'s own existing logic already makes the first upload for a
/// holding primary and every later one non-primary, so this needs no
/// caller-side special-casing; `PhotoGallery` below is where a collector
/// manages which one is primary or removes one, not this control.
#[component]
pub fn PhotoCapture(holding_id: i64, on_uploaded: EventHandler<HoldingImage>) -> Element {
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
                        .run(move |repo| repo.add_photo(holding_id, &bytes, PhotoStorage::Inline))
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

/// Computes the new photo order after dragging `dragged_id` onto
/// `target_id`'s tile - removes `dragged_id` from `current`, then
/// reinserts it right where `target_id` now sits. A pure function so the
/// actual reorder math is testable without a DOM/drag event at all.
/// Dragging a tile onto itself is a no-op, not an append-to-the-end bug.
fn reorder_ids(current: &[i64], dragged_id: i64, target_id: i64) -> Vec<i64> {
    if dragged_id == target_id {
        return current.to_vec();
    }
    let mut ids: Vec<i64> = current
        .iter()
        .copied()
        .filter(|&id| id != dragged_id)
        .collect();
    let target_index = ids
        .iter()
        .position(|&id| id == target_id)
        .unwrap_or(ids.len());
    ids.insert(target_index, dragged_id);
    ids
}

/// Every photo on a holding, as a small grid of tiles - used both on Card
/// Details (under its edit-mode toggle, alongside `PhotoCapture`) and
/// right on the Buy success screen (so a photo added at Buy-time is
/// actually visible and removable there, not just added into the void).
/// Each tile: drag to reorder (native HTML5 drag-and-drop, no
/// third-party crate - Dioxus 0.7 wires `ondragstart`/`ondragover`/
/// `ondrop` straight to the real DOM on web), click a non-primary tile
/// to make it primary, hover to reveal a delete-X.
#[component]
pub fn PhotoGallery(
    holding_id: i64,
    photos: Vec<HoldingImage>,
    on_changed: EventHandler<()>,
) -> Element {
    let bridge = use_context::<WebBridge>();
    let mut dragging_id = use_signal(|| None::<i64>);
    let ids: Vec<i64> = photos.iter().map(|p| p.id).collect();

    rsx! {
        div { class: "flex flex-wrap gap-2",
            for photo in photos.iter().cloned() {
                div {
                    key: "{photo.id}",
                    class: if photo.is_primary {
                        "group relative w-16 h-16 shrink-0 rounded-lg overflow-hidden ring-2 ring-gold cursor-grab"
                    } else {
                        "group relative w-16 h-16 shrink-0 rounded-lg overflow-hidden cursor-grab"
                    },
                    draggable: "true",
                    ondragstart: {
                        let photo_id = photo.id;
                        move |_| dragging_id.set(Some(photo_id))
                    },
                    ondragover: move |evt| evt.prevent_default(),
                    ondrop: {
                        let bridge = bridge.clone();
                        let ids = ids.clone();
                        let target_id = photo.id;
                        move |_| {
                            let Some(dragged_id) = dragging_id() else {
                                return;
                            };
                            dragging_id.set(None);
                            if dragged_id == target_id {
                                return;
                            }
                            let bridge = bridge.clone();
                            let new_order = reorder_ids(&ids, dragged_id, target_id);
                            spawn(async move {
                                let _ = bridge
                                    .run(move |repo| repo.reorder_photos(holding_id, &new_order))
                                    .await;
                                on_changed.call(());
                            });
                        }
                    },
                    onclick: {
                        let bridge = bridge.clone();
                        let photo_id = photo.id;
                        let is_primary = photo.is_primary;
                        move |_| {
                            if is_primary {
                                return;
                            }
                            let bridge = bridge.clone();
                            spawn(async move {
                                let _ = bridge
                                    .run(move |repo| repo.set_primary_photo(holding_id, photo_id))
                                    .await;
                                on_changed.call(());
                            });
                        }
                    },
                    img {
                        class: "w-full h-full object-cover pointer-events-none",
                        src: "data:image/jpeg;base64,{BASE64.encode(&photo.thumbnail_data)}",
                    }
                    button {
                        class: "absolute top-0.5 right-0.5 w-5 h-5 flex items-center justify-center rounded-full bg-canvas/70 text-text-tertiary opacity-0 group-hover:opacity-100 border-none cursor-pointer transition-opacity duration-[var(--duration-standard)] ease-standard hover:text-loss",
                        "aria-label": "Remove photo",
                        onclick: {
                            let bridge = bridge.clone();
                            let photo_id = photo.id;
                            move |evt: MouseEvent| {
                                evt.stop_propagation();
                                let bridge = bridge.clone();
                                spawn(async move {
                                    let _ = bridge
                                        .run(move |repo| repo.delete_photo(photo_id, PhotoStorage::Inline))
                                        .await;
                                    on_changed.call(());
                                });
                            }
                        },
                        Icon { icon: LdX, width: 12, height: 12 }
                    }
                }
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

    #[wasm_bindgen_test]
    fn reorder_ids_moves_the_dragged_id_to_the_target_and_closes_the_gap() {
        assert_eq!(reorder_ids(&[1, 2, 3], 1, 3), vec![2, 1, 3]);
        assert_eq!(reorder_ids(&[1, 2, 3], 3, 1), vec![3, 1, 2]);
        assert_eq!(reorder_ids(&[1, 2, 3, 4], 4, 2), vec![1, 4, 2, 3]);
    }

    #[wasm_bindgen_test]
    fn reorder_ids_dropping_a_tile_onto_itself_is_a_no_op() {
        assert_eq!(reorder_ids(&[1, 2, 3], 2, 2), vec![1, 2, 3]);
    }
}
