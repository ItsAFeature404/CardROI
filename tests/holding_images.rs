//! Integration tests for `Repository::add_photo`/`delete_photo`/
//! `delete_holding_with_images`/`set_primary_photo` against a real
//! in-memory database and a real (temp-directory) filesystem - no mocks,
//! matching this project's "prefer real implementations" testing habit.
//! Test images are generated in-process via the `image` crate rather than
//! checked-in binary fixtures.
//!
//! `PhotoStorage::Disk` is exercised throughout (matching native/CLI's
//! real behavior); a separate block of tests near the bottom covers
//! `PhotoStorage::Inline` (`cardroi-web`'s behavior - no filesystem, full
//! bytes stored directly in the row).

use std::io::Cursor;
use std::path::Path;

use cardroi::db::open_in_memory;
use cardroi::db::repository::{PhotoStorage, Repository};
use cardroi::models::{NewCard, NewHolding, NewSet};
use tempfile::tempdir;

fn repo() -> Repository {
    Repository::new(open_in_memory().expect("in-memory db should open"))
}

fn seed_holding(repo: &Repository) -> i64 {
    let set = repo
        .create_set(&NewSet {
            name: "2023 Topps Chrome".to_string(),
            sport: "Basketball".to_string(),
            ..Default::default()
        })
        .unwrap();
    let card = repo
        .create_card(&NewCard {
            set_id: set.id,
            card_number: "123".to_string(),
            player_name: "LeBron James".to_string(),
            ..Default::default()
        })
        .unwrap();
    repo.create_holding(&NewHolding {
        card_id: card.id,
        ..Default::default()
    })
    .unwrap()
    .id
}

/// A real, decodable JPEG of the given size, solid-colored - enough to
/// exercise decode/resize/re-encode without a checked-in binary fixture.
fn fake_jpeg(width: u32, height: u32) -> Vec<u8> {
    fake_jpeg_colored(width, height, [200, 50, 50])
}

/// Same as `fake_jpeg`, but with a caller-chosen solid color - for tests
/// that need two genuinely distinguishable photos (a resized thumbnail
/// of two same-color, differently-sized solid images is byte-identical,
/// which would make an equality assertion meaningless).
fn fake_jpeg_colored(width: u32, height: u32, color: [u8; 3]) -> Vec<u8> {
    let img = image::ImageBuffer::from_pixel(width, height, image::Rgb(color));
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 90)
        .encode(&img, width, height, image::ExtendedColorType::Rgb8)
        .unwrap();
    bytes
}

fn count_files_in(dir: &Path) -> usize {
    if !dir.exists() {
        return 0;
    }
    walk_count(dir)
}

fn walk_count(dir: &Path) -> usize {
    let mut count = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            count += walk_count(&path);
        } else {
            count += 1;
        }
    }
    count
}

#[test]
fn add_photo_makes_the_first_upload_primary_and_position_zero() {
    let repo = repo();
    let holding_id = seed_holding(&repo);
    let images_root = tempdir().unwrap();

    let photo = repo
        .add_photo(
            holding_id,
            &fake_jpeg(400, 300),
            PhotoStorage::Disk(images_root.path()),
        )
        .unwrap();

    assert!(photo.is_primary);
    assert_eq!(photo.position, 0);
    assert_eq!(photo.width, 400);
    assert_eq!(photo.height, 300);
    assert!(!photo.thumbnail_data.is_empty());
    assert!(photo.file_path.is_some());
    assert!(photo.full_data.is_none());
}

#[test]
fn get_photo_bytes_reads_the_full_size_file_not_the_small_thumbnail() {
    let repo = repo();
    let holding_id = seed_holding(&repo);
    let images_root = tempdir().unwrap();

    let photo = repo
        .add_photo(
            holding_id,
            &fake_jpeg(800, 600),
            PhotoStorage::Disk(images_root.path()),
        )
        .unwrap();

    let full_bytes = repo
        .get_photo_bytes(photo.id, PhotoStorage::Disk(images_root.path()))
        .unwrap();
    let full_image = image::load_from_memory(&full_bytes).unwrap();
    assert_eq!((full_image.width(), full_image.height()), (800, 600));
    // The full-size file is meaningfully larger than the thumbnail -
    // proof this reads the real on-disk original, not the row's small
    // preview BLOB.
    assert!(full_bytes.len() > photo.thumbnail_data.len());
}

#[test]
fn uploading_identical_bytes_twice_creates_two_rows_but_writes_the_file_once() {
    let repo = repo();
    let holding_id = seed_holding(&repo);
    let images_root = tempdir().unwrap();
    let bytes = fake_jpeg(400, 300);

    let first = repo
        .add_photo(holding_id, &bytes, PhotoStorage::Disk(images_root.path()))
        .unwrap();
    let second = repo
        .add_photo(holding_id, &bytes, PhotoStorage::Disk(images_root.path()))
        .unwrap();

    assert_eq!(first.file_hash, second.file_hash);
    assert_ne!(first.id, second.id);
    assert_eq!(count_files_in(images_root.path()), 1);

    let all = repo.list_photos_for_holding(holding_id).unwrap();
    assert_eq!(all.len(), 2);
    // Only the first upload is primary - the second is a later position.
    assert!(all.iter().filter(|p| p.is_primary).count() == 1);
}

#[test]
fn oversized_images_are_capped_to_the_long_edge_limit() {
    let repo = repo();
    let holding_id = seed_holding(&repo);
    let images_root = tempdir().unwrap();

    let photo = repo
        .add_photo(
            holding_id,
            &fake_jpeg(3000, 1500),
            PhotoStorage::Disk(images_root.path()),
        )
        .unwrap();

    assert_eq!(photo.width, 2000);
    assert_eq!(photo.height, 1000);
}

#[test]
fn an_undecodable_upload_is_rejected_cleanly_not_a_panic() {
    let repo = repo();
    let holding_id = seed_holding(&repo);
    let images_root = tempdir().unwrap();

    let err = repo
        .add_photo(
            holding_id,
            b"not an image",
            PhotoStorage::Disk(images_root.path()),
        )
        .unwrap_err();
    assert!(err.to_string().contains("couldn't"));
    assert!(repo.list_photos_for_holding(holding_id).unwrap().is_empty());
    assert_eq!(count_files_in(images_root.path()), 0);
}

#[test]
fn deleting_one_of_two_rows_sharing_a_hash_keeps_the_file_deleting_the_last_removes_it() {
    let repo = repo();
    let holding_a = seed_holding(&repo);
    let holding_b = seed_holding(&repo);
    let images_root = tempdir().unwrap();
    let bytes = fake_jpeg(400, 300);

    let photo_a = repo
        .add_photo(holding_a, &bytes, PhotoStorage::Disk(images_root.path()))
        .unwrap();
    let photo_b = repo
        .add_photo(holding_b, &bytes, PhotoStorage::Disk(images_root.path()))
        .unwrap();
    assert_eq!(count_files_in(images_root.path()), 1);

    repo.delete_photo(photo_a.id, PhotoStorage::Disk(images_root.path()))
        .unwrap();
    assert_eq!(
        count_files_in(images_root.path()),
        1,
        "file must survive while photo_b still references its hash"
    );

    repo.delete_photo(photo_b.id, PhotoStorage::Disk(images_root.path()))
        .unwrap();
    assert_eq!(
        count_files_in(images_root.path()),
        0,
        "file must be removed once nothing references it"
    );
}

#[test]
fn deleting_the_primary_photo_promotes_the_next_one() {
    let repo = repo();
    let holding_id = seed_holding(&repo);
    let images_root = tempdir().unwrap();

    let first = repo
        .add_photo(
            holding_id,
            &fake_jpeg(400, 300),
            PhotoStorage::Disk(images_root.path()),
        )
        .unwrap();
    let second = repo
        .add_photo(
            holding_id,
            &fake_jpeg(300, 400),
            PhotoStorage::Disk(images_root.path()),
        )
        .unwrap();
    assert!(first.is_primary);

    repo.delete_photo(first.id, PhotoStorage::Disk(images_root.path()))
        .unwrap();

    let remaining = repo.get_photo(second.id).unwrap();
    assert!(
        remaining.is_primary,
        "the only remaining photo must be promoted to primary"
    );
}

#[test]
fn deleting_the_only_photo_leaves_no_primary_behind() {
    let repo = repo();
    let holding_id = seed_holding(&repo);
    let images_root = tempdir().unwrap();

    let only = repo
        .add_photo(
            holding_id,
            &fake_jpeg(400, 300),
            PhotoStorage::Disk(images_root.path()),
        )
        .unwrap();
    repo.delete_photo(only.id, PhotoStorage::Disk(images_root.path()))
        .unwrap();

    assert!(repo.list_photos_for_holding(holding_id).unwrap().is_empty());
}

#[test]
fn set_primary_photo_leaves_exactly_one_primary() {
    let repo = repo();
    let holding_id = seed_holding(&repo);
    let images_root = tempdir().unwrap();

    let first = repo
        .add_photo(
            holding_id,
            &fake_jpeg(400, 300),
            PhotoStorage::Disk(images_root.path()),
        )
        .unwrap();
    let second = repo
        .add_photo(
            holding_id,
            &fake_jpeg(300, 400),
            PhotoStorage::Disk(images_root.path()),
        )
        .unwrap();
    assert!(first.is_primary && !second.is_primary);

    repo.set_primary_photo(holding_id, second.id).unwrap();

    let photos = repo.list_photos_for_holding(holding_id).unwrap();
    assert_eq!(photos.iter().filter(|p| p.is_primary).count(), 1);
    assert!(
        photos
            .iter()
            .find(|p| p.id == second.id)
            .unwrap()
            .is_primary
    );
}

#[test]
fn get_primary_thumbnail_returns_none_for_a_photo_less_holding() {
    let repo = repo();
    let holding_id = seed_holding(&repo);

    assert!(repo.get_primary_thumbnail(holding_id).unwrap().is_none());
}

#[test]
fn get_primary_thumbnail_returns_the_primarys_thumbnail_specifically() {
    let repo = repo();
    let holding_id = seed_holding(&repo);
    let images_root = tempdir().unwrap();

    let first = repo
        .add_photo(
            holding_id,
            &fake_jpeg_colored(400, 300, [200, 50, 50]),
            PhotoStorage::Disk(images_root.path()),
        )
        .unwrap();
    let second = repo
        .add_photo(
            holding_id,
            &fake_jpeg_colored(300, 400, [50, 50, 200]),
            PhotoStorage::Disk(images_root.path()),
        )
        .unwrap();
    repo.set_primary_photo(holding_id, second.id).unwrap();

    let thumbnail = repo.get_primary_thumbnail(holding_id).unwrap().unwrap();
    assert_eq!(thumbnail, second.thumbnail_data);
    assert_ne!(thumbnail, first.thumbnail_data);
}

#[test]
fn reorder_photos_persists_the_new_order() {
    let repo = repo();
    let holding_id = seed_holding(&repo);
    let images_root = tempdir().unwrap();

    let first = repo
        .add_photo(
            holding_id,
            &fake_jpeg(400, 300),
            PhotoStorage::Disk(images_root.path()),
        )
        .unwrap();
    let second = repo
        .add_photo(
            holding_id,
            &fake_jpeg(300, 400),
            PhotoStorage::Disk(images_root.path()),
        )
        .unwrap();
    let third = repo
        .add_photo(
            holding_id,
            &fake_jpeg(500, 500),
            PhotoStorage::Disk(images_root.path()),
        )
        .unwrap();

    // Reverse the insertion order, keeping `first` (the primary) last.
    repo.reorder_photos(holding_id, &[third.id, second.id, first.id])
        .unwrap();

    // list_photos_for_holding orders primary DESC first, so `first`
    // (still primary) leads regardless of its new `position` - the
    // reorder is only visible among the non-primary photos here.
    let photos = repo.list_photos_for_holding(holding_id).unwrap();
    assert_eq!(photos[0].id, first.id, "primary still sorts first");
    assert_eq!(
        photos[1..].iter().map(|p| p.id).collect::<Vec<_>>(),
        vec![third.id, second.id],
        "non-primary photos reflect the new order"
    );
}

#[test]
fn reorder_photos_rejects_an_id_not_belonging_to_the_holding() {
    let repo = repo();
    let holding_a = seed_holding(&repo);
    let holding_b = seed_holding(&repo);
    let images_root = tempdir().unwrap();

    let photo_a = repo
        .add_photo(
            holding_a,
            &fake_jpeg(400, 300),
            PhotoStorage::Disk(images_root.path()),
        )
        .unwrap();
    let photo_b = repo
        .add_photo(
            holding_b,
            &fake_jpeg(400, 300),
            PhotoStorage::Disk(images_root.path()),
        )
        .unwrap();

    let err = repo
        .reorder_photos(holding_a, &[photo_a.id, photo_b.id])
        .unwrap_err();
    assert!(err.to_string().contains("holding_image"));

    // Nothing should have been persisted from the failed reorder - not
    // even photo_a's own position change, since this is one transaction.
    let photos = repo.list_photos_for_holding(holding_a).unwrap();
    assert_eq!(photos[0].position, 0);
}

#[test]
fn deleting_a_holding_cascades_its_photo_rows_and_cleans_up_unreferenced_files() {
    let repo = repo();
    let holding_id = seed_holding(&repo);
    let images_root = tempdir().unwrap();

    repo.add_photo(
        holding_id,
        &fake_jpeg(400, 300),
        PhotoStorage::Disk(images_root.path()),
    )
    .unwrap();
    repo.add_photo(
        holding_id,
        &fake_jpeg(300, 400),
        PhotoStorage::Disk(images_root.path()),
    )
    .unwrap();
    assert_eq!(count_files_in(images_root.path()), 2);

    repo.delete_holding_with_images(holding_id, PhotoStorage::Disk(images_root.path()))
        .unwrap();

    assert!(repo.list_photos_for_holding(holding_id).unwrap().is_empty());
    assert_eq!(
        count_files_in(images_root.path()),
        0,
        "no other holding references these hashes, so both files must be cleaned up"
    );
}

#[test]
fn deleting_a_holding_does_not_remove_a_file_still_referenced_by_another_holding() {
    let repo = repo();
    let holding_a = seed_holding(&repo);
    let holding_b = seed_holding(&repo);
    let images_root = tempdir().unwrap();
    let bytes = fake_jpeg(400, 300);

    repo.add_photo(holding_a, &bytes, PhotoStorage::Disk(images_root.path()))
        .unwrap();
    repo.add_photo(holding_b, &bytes, PhotoStorage::Disk(images_root.path()))
        .unwrap();

    repo.delete_holding_with_images(holding_a, PhotoStorage::Disk(images_root.path()))
        .unwrap();

    assert_eq!(
        count_files_in(images_root.path()),
        1,
        "holding_b's photo still references this hash"
    );
    assert_eq!(repo.list_photos_for_holding(holding_b).unwrap().len(), 1);
}

#[test]
fn png_uploads_are_accepted_and_normalized_to_jpeg() {
    let repo = repo();
    let holding_id = seed_holding(&repo);
    let images_root = tempdir().unwrap();

    let img = image::ImageBuffer::from_pixel(200, 200, image::Rgb([10u8, 20, 30]));
    let mut png_bytes = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut Cursor::new(&mut png_bytes), image::ImageFormat::Png)
        .unwrap();

    let photo = repo
        .add_photo(
            holding_id,
            &png_bytes,
            PhotoStorage::Disk(images_root.path()),
        )
        .unwrap();
    assert_eq!(photo.mime_type, "image/jpeg");
    assert_eq!(photo.width, 200);
    assert_eq!(photo.height, 200);
}

// --- PhotoStorage::Inline (cardroi-web's storage mode - no filesystem) ---

#[test]
fn inline_storage_stores_full_bytes_in_the_row_not_on_disk() {
    let repo = repo();
    let holding_id = seed_holding(&repo);

    let photo = repo
        .add_photo(holding_id, &fake_jpeg(400, 300), PhotoStorage::Inline)
        .unwrap();

    assert!(photo.file_path.is_none());
    let full_data = photo
        .full_data
        .expect("inline storage must populate full_data");
    let full_image = image::load_from_memory(&full_data).unwrap();
    assert_eq!((full_image.width(), full_image.height()), (400, 300));
}

#[test]
fn inline_storage_caps_to_a_smaller_long_edge_than_disk_storage() {
    let repo = repo();
    let holding_id = seed_holding(&repo);

    let photo = repo
        .add_photo(holding_id, &fake_jpeg(3000, 1500), PhotoStorage::Inline)
        .unwrap();

    assert_eq!(photo.width, 1200);
    assert_eq!(photo.height, 600);
}

#[test]
fn get_photo_bytes_returns_the_rows_own_full_data_under_inline_storage() {
    let repo = repo();
    let holding_id = seed_holding(&repo);

    let photo = repo
        .add_photo(holding_id, &fake_jpeg(400, 300), PhotoStorage::Inline)
        .unwrap();

    let bytes = repo
        .get_photo_bytes(photo.id, PhotoStorage::Inline)
        .unwrap();
    assert_eq!(Some(bytes), photo.full_data);
}

#[test]
fn delete_photo_under_inline_storage_needs_no_real_directory() {
    let repo = repo();
    let holding_id = seed_holding(&repo);

    let photo = repo
        .add_photo(holding_id, &fake_jpeg(400, 300), PhotoStorage::Inline)
        .unwrap();

    repo.delete_photo(photo.id, PhotoStorage::Inline).unwrap();

    assert!(repo.list_photos_for_holding(holding_id).unwrap().is_empty());
}

#[test]
fn deleting_a_holding_cascade_alone_is_enough_to_clean_up_inline_photos() {
    // No `delete_holding_with_images` call here at all - this is the
    // point: `ON DELETE CASCADE` already removes the `holding_images`
    // row, which is the only place an inline photo's bytes ever lived.
    let repo = repo();
    let holding_id = seed_holding(&repo);

    repo.add_photo(holding_id, &fake_jpeg(400, 300), PhotoStorage::Inline)
        .unwrap();

    repo.delete_holding_cascade(holding_id).unwrap();

    assert!(repo.list_photos_for_holding(holding_id).unwrap().is_empty());
}
