//! Qiyas core: the document, selection, undo and redo, command bus, PDF import and underlays,
//! raster images, inquiry (geometry, divide and measure along, data extraction), drawing new
//! entities and modifying them, text and find and replace, dimensions, leaders and tolerances,
//! tables and fields, blocks and attributes, layer properties, layer states and the layer
//! translator, groups, editing and matching entity properties, the clipboard, paper-space
//! viewports, the user coordinate system, sheet sets, autosave and recovery, and parametric
//! constraints.

pub mod audit;
mod autosave;
mod blocks;
mod boq;
mod clipboard;
mod commands;
mod comment;
mod conflict;
mod dimension;
mod draw;
mod drawing;
mod embed;
mod error;
mod field;
#[cfg(any(test, feature = "fixtures"))]
pub mod fixtures;
mod groups;
mod image;
mod inquiry;
mod layers;
mod modify;
mod modify2;
mod param;
mod project;
mod props;
pub mod purge;
mod query;
pub mod record_edit;
pub mod records;
pub mod registry;
pub mod script;
mod session;
mod sheetset;
mod snap;
mod table;
mod tables;
mod takeoff;
mod text;
mod transaction;
mod ucs;
mod underlay;
mod undo;
mod units;
mod viewports;
mod views;
mod xref;

pub use self::image::{draw_picture, image_size, ImageDisplay, IMAGE_EXTENSIONS, IMAGE_LAYER};
pub use autosave::{
    autosave_all, autosave_copy_of, autosave_path, discard_autosave, discard_recoverable,
    lock_path, read_lock, recoverable, refuse_if_open_elsewhere, release_lock, take_lock,
    AutosaveRun, CopyKind, DrawingLock, Dropped, LockStatus, OlderCopy, ReadReport, Recoverable,
    AUTOSAVE_EVERY, CRASH_COPIES,
};
pub use blocks::{drawing_blocks, AttDefInfo, BlockInfo};
pub use boq::{Boq, BoqItem};
pub use clipboard::CLIPBOARD_MARK;
pub use commands::{Command, Outcome, Target};
pub use comment::{CommentInfo, COMMENT_APP, COMMENT_LAYER};
pub use conflict::{Conflict, ConflictKind, ConflictStatus};
pub use dimension::{DimEditOp, DimSpec, DimStyleFields, DimStyleInfo};
pub use draw::{Boundary, Current, DrawSpec, HatchPattern, BOUNDARY_SEGMENTS};
pub use drawing::{short_layer_name, Drawing, LayerInfo, Sheet};
pub use embed::{embed_measurements, read_embedded};
pub use error::CoreError;
pub use field::{field_code, FieldKind, FIELD_APP};
pub use groups::GroupInfo;
pub use inquiry::{Field, POINT_LAYER};
pub use layers::{drawing_layers, LayerMapping, LayerProp, ViewportInfo};
pub use modify::ModifySpec;
pub use modify2::{EditSpec, LengthenBy, PEditOp};
pub use param::{
    AutoSettings, ConstraintInfo, ConstraintKind, ParameterInfo, Part as ConstraintPart, Ref,
    PARAM_APP,
};
pub use project::{project_path, Project};
pub use props::{EntityProp, MatchProp};
pub use query::{BlockCount, DimRow, EntityFilter, EntityRow, Page, TextRow};
pub use record_edit::{NewEntity, RecordUpdate};
pub use script::Script;
pub use session::{
    batch_tol_for, zoom_tol, DrawingInfo, EntityInfo, Picked, Session, BATCH_TOL, EMPTY_MODEL,
    UNTITLED, ZOOM_TOL_PX,
};
pub use sheetset::{sheet_set_entries, sheet_set_entry, SheetSet, SheetSetDrawing, SheetSetEntry};
pub use snap::{Drafting, Snap, SnapKind, SnapModes};
pub use table::TableInfo;
pub use tables::Schedule;
pub use takeoff::{
    AreaOrigin, Kind, Measurement, MeasurementBasis, Source, Takeoff, Total,
    INSERT_COUNT_LIMITATIONS, INSERT_COUNT_SCOPE,
};
pub use text::{TextMatch, TextScope};
pub use transaction::{Change, HistoryStep};
pub use ucs::Ucs;
pub use underlay::{AttachMode, PictureSource, PreparedPdf, UnderlayImage, UNDERLAY_LAYER};
pub use units::Units;
pub use viewports::PaperView;
pub use views::NamedView;
pub use xref::{candidates as xref_candidates, Xref, XrefInfo, XrefSet};
