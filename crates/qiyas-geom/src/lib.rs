//! Pure 2D geometry: flatten CAD entities to polylines, measure length and area.

mod dimension;
mod exact;
mod flatten;
mod paint;
mod path;
mod shapes;
mod table;
mod transform;

pub use dimension::{
    clear_dimpost, dimension_entities, dimension_text, dimpost, dimscale, dimtxt, format_measure,
    leader_entities, set_dimpost, set_dimscale, set_dimtxt, style_named, tolerance_entities,
    DimGeometry, DimLook,
};
pub use exact::{exact_area, exact_hatch_areas, exact_length};
pub use flatten::{
    arc_points, drawn_style, find_layer, flatten, flatten_with, flatten_within, insert_instances,
    is_logical_text, is_overlay_block, is_xref_block, object_to_world, xref_symbol, FlatPath,
    FlatText, FlattenOutcome, HAlign, TextLayout, VAlign, XrefDocs, LOGICAL_TEXT_APP, MAX_WORK,
    MAX_XREF_DEPTH,
};
pub use paint::{FlatFill, Paint};
pub use path::Path;
pub use shapes::{snap_shapes, SnapShape};
pub use table::{fit_rows, merge_owners, table_block, table_cell_at, table_entities, table_xform};
pub use transform::Xform;
