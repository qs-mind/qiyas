//! Turns a sheet of the document into GPU buffers (line segments, fill triangles, glyph quads over
//! a glyph atlas, image quads), and picks entities near a point.

mod atlas;
mod batch;
mod color;
mod fonts;
mod index;
mod text;
mod visibility;

pub use atlas::{Atlas, PageView, PAGE_SIZE};
pub use batch::{
    add_images, add_pick_outlines, build_batch, build_batch_with_text, build_batch_with_xrefs,
    image_flags, place_text, section, Batch, ImageEntry, ImagePlacement, ImageQuad, Range,
    TextOptions, TileSpan, Visibility, FILL_BACKGROUND, IMAGE_ADJUSTED, IMAGE_FADED,
    IMAGE_MONOCHROME, MAGIC, MAX_SHEET_WORK, NO_CHAIN, PATTERN, TEXT_BOX, VERSION,
};
pub use color::resolve_color;
pub use fonts::{DEFAULT_CHAIN, NOTO_ARABIC, NOTO_SANS};
pub use index::PickIndex;
pub use text::{
    arabic_stored_visually, decode_codes, size_bucket, visual_pieces, BaseDirection, Laid,
    LaidText, Quad, ShapedGlyph, ShapedLine, TextEngine, VisualPiece, CAP_EM, QUAD_IN_BLOCK,
    SIZE_BUCKETS,
};
pub use visibility::{layer_on_in_file, layer_on_in_file_with, layer_shown, layer_shown_with};
