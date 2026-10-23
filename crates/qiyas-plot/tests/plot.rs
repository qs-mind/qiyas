//! Plot to PDF: what the screen draws, at the plot scale, read back from the PDF with lopdf.

mod common;

use common::{close, near, read, PT_PER_MM};
use opencadcodec::{CadDocument, EntityType, LwPolyline, Vector2};
use qiyas_core::fixtures::{arabic_text_doc, write_drawing, write_sample};
use qiyas_core::{Command, DrawSpec, EntityProp, HatchPattern, Outcome, Session, Target};
use qiyas_plot::{
    paper_catalog, plot_pdf, plot_pdf_to, plot_snapshot, write_pdf, Area, Orientation, Paper,
    PlotError, PlotJob, PlotStyle, Scale,
};
use serde_json::json;

const MODEL: &str = "*Model_Space";

fn paper(name: &str) -> Paper {
    paper_catalog()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("{name} in the catalog"))
}

fn job(sheet: &str) -> PlotJob {
    PlotJob {
        sheet: sheet.to_string(),
        area: Area::Extents,
        paper: paper("ISO A3"),
        orientation: Orientation::Landscape,
        scale: Scale::Fit,
        centre: true,
        style: None,
        lineweights: true,
        output: None,
    }
}

fn sample() -> (tempfile::TempDir, Session) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_sample(dir.path(), "dxf");
    (dir, Session::open(&path).unwrap())
}

fn t(handle: u64) -> Target {
    Target {
        handle,
        chain: Vec::new(),
    }
}

/// Page points of drawing point `p` for a window from `lo`, not centred, at `mm` page mm per
/// drawing unit.
fn page_of(p: [f64; 2], lo: [f64; 2], mm: f64) -> [f32; 2] {
    [
        ((p[0] - lo[0]) * mm) as f32 * PT_PER_MM,
        ((p[1] - lo[1]) * mm) as f32 * PT_PER_MM,
    ]
}

/// Review Focus 1: a layout with a paper-space viewport at 1:100 (10 paper mm per metre) on A1:
/// the sample's 10 m wall line measures 100 mm on the page, where the viewport puts it.
#[test]
fn a_10_m_line_in_a_1_to_100_viewport_measures_100_mm_on_a1() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::open(&write_sample(dir.path(), "dwg")).unwrap();
    s.run(Command::NewLayer {
        name: "PLOT-VIEWPORT".to_string(),
    })
    .unwrap();
    s.current.layer = "PLOT-VIEWPORT".to_string();
    assert!(
        s.drawing
            .doc
            .layers
            .get("PLOT-VIEWPORT")
            .unwrap()
            .is_plottable
    );
    let layout = s
        .drawing
        .sheets()
        .into_iter()
        .find(|sh| !sh.is_model)
        .unwrap();
    s.set_sheet(&layout.block).unwrap();
    let Outcome::Added { handles } = s
        .run(Command::MView {
            model_bounds: None,
            paper_centre: None,
            target: None,
            scale: None,
            centre: None,
            corner1: [20.0, 20.0],
            corner2: [820.0, 570.0],
        })
        .unwrap()
    else {
        panic!("a viewport");
    };
    s.run(Command::SetProperty {
        target: t(handles[0]),
        prop: EntityProp::ViewportScale,
        value: json!(10.0),
    })
    .unwrap();
    let view = s.paper_views()[0].clone();
    assert_eq!(view.scale, 10.0);

    let mut j = job(&layout.block);
    j.paper = paper("ISO A1");
    j.scale = Scale::Ratio(1.0, 1.0);
    j.output = Some("layout.pdf".to_string());
    let out = plot_pdf(&s, &j).unwrap();
    assert_eq!(
        out.path,
        dir.path().canonicalize().unwrap().join("layout.pdf")
    );
    let page = read(&out.path);
    assert!(
        close(page.size[0], 841.0 * PT_PER_MM, 0.01),
        "{:?}",
        page.size
    );
    assert!(
        close(page.size[1], 594.0 * PT_PER_MM, 0.01),
        "{:?}",
        page.size
    );

    // Paper point of a model point, then the page: the layout's extents (the frame) centred.
    let paper_of = |m: [f64; 2]| {
        [
            420.0 + (m[0] - view.centre[0]) * 10.0,
            295.0 + (m[1] - view.centre[1]) * 10.0,
        ]
    };
    let shift = [(841.0 - 800.0) / 2.0 - 20.0, (594.0 - 550.0) / 2.0 - 20.0];
    let on_page = |m: [f64; 2]| {
        let p = paper_of(m);
        [
            ((p[0] + shift[0]) as f32) * PT_PER_MM,
            ((p[1] + shift[1]) as f32) * PT_PER_MM,
        ]
    };
    let (a, b) = (on_page([0.0, 0.0]), on_page([10.0, 0.0]));
    let wall = page
        .strokes()
        .into_iter()
        .find(|(_, p, q)| near(*p, a, 0.05) && near(*q, b, 0.05))
        .unwrap_or_else(|| panic!("the wall line from {a:?} to {b:?}"));
    let length = (wall.2[0] - wall.1[0]).hypot(wall.2[1] - wall.1[1]);
    assert!(close(length, 100.0 * PT_PER_MM, 0.05), "{length} pt");
    assert!(wall.0.clipped, "drawn inside the viewport's clip");
    assert_eq!(wall.0.stroke, [1.0, 0.0, 0.0], "red, as A-WALL draws");
    // The frame itself draws on the layout, 800 mm wide.
    assert!(page.strokes().iter().any(|(_, p, q)| {
        close((q[0] - p[0]).abs(), 800.0 * PT_PER_MM, 0.05) && close(p[1], q[1], 0.01)
    }));
}

/// The default viewport layer hides its frame on paper while model geometry still plots.
#[test]
fn a_viewport_on_a_view_plots_model_geometry_without_its_frame() {
    let (dir, mut s) = sample();
    assert_eq!(s.current.layer, "0");
    let layout = s
        .drawing
        .sheets()
        .into_iter()
        .find(|sh| !sh.is_model)
        .unwrap();
    s.set_sheet(&layout.block).unwrap();
    let Outcome::Added { handles } = s
        .run(Command::MView {
            model_bounds: None,
            paper_centre: None,
            target: None,
            scale: None,
            centre: None,
            corner1: [20.0, 20.0],
            corner2: [820.0, 570.0],
        })
        .unwrap()
    else {
        panic!("a viewport");
    };
    let viewport = s
        .drawing
        .doc
        .get_entity(opencadcodec::Handle::new(handles[0]))
        .unwrap();
    assert_eq!(viewport.common().layer, "A-VIEW");
    assert!(!s.drawing.doc.layers.get("A-VIEW").unwrap().is_plottable);
    let mut j = job(&layout.block);
    j.paper = paper("ISO A1");
    j.scale = Scale::Ratio(1.0, 1.0);
    let out = plot_pdf_to(&s, &j, &dir.path().join("no-frame.pdf")).unwrap();
    let page = read(&out.path);
    let strokes = page.strokes();
    assert!(!strokes.is_empty(), "model geometry still plots");
    assert!(
        strokes.iter().all(|(paint, _, _)| paint.clipped),
        "only model geometry inside the viewport, with no paper-space frame"
    );
    assert!(!strokes.iter().any(|(_, p, q)| {
        close((q[0] - p[0]).abs(), 800.0 * PT_PER_MM, 0.05) && close(p[1], q[1], 0.01)
    }));
}

/// Model space at 1:100 in metres: 10 m is 100 mm, from the window's lower left.
#[test]
fn model_space_at_1_to_100_puts_10_m_at_100_mm() {
    let (dir, s) = sample();
    let mut j = job(MODEL);
    j.area = Area::Window([[-5.0, -5.0], [25.0, 5.0]]);
    j.scale = Scale::Ratio(1.0, 100.0);
    j.centre = false;
    let out = plot_pdf_to(&s, &j, &dir.path().join("model.pdf")).unwrap();
    let page = read(&out.path);
    let (a, b) = (
        page_of([0.0, 0.0], [-5.0, -5.0], 10.0),
        page_of([10.0, 0.0], [-5.0, -5.0], 10.0),
    );
    assert!(
        page.strokes()
            .iter()
            .any(|(_, p, q)| near(*p, a, 0.05) && near(*q, b, 0.05)),
        "the wall from {a:?} to {b:?}"
    );
}

/// Review Focus 2: a CTB maps colour 1 to black at 0.5 mm and colour 3 to no plot: the red
/// walls plot black at 0.5 mm (1.4173 pt) and nothing green is on the page.
#[test]
fn a_ctb_plots_red_black_at_half_a_millimetre_and_leaves_green_out() {
    let (dir, s) = sample();
    let mut ctb = PlotStyle::default_ctb();
    ctb.styles[0].color = Some([0, 0, 0]);
    ctb.styles[0].lineweight = 13;
    assert_eq!(ctb.lineweights[13], 0.5);
    ctb.styles[2].screen = 0;
    let mut j = job(MODEL);
    j.area = Area::Window([[-5.0, -5.0], [25.0, 5.0]]);
    j.scale = Scale::Ratio(1.0, 100.0);
    j.centre = false;
    j.style = Some(ctb);
    let out = plot_pdf_to(&s, &j, &dir.path().join("ctb.pdf")).unwrap();
    let page = read(&out.path);
    let strokes = page.strokes();
    let at = |p: [f64; 2]| page_of(p, [-5.0, -5.0], 10.0);
    let wall = strokes
        .iter()
        .find(|(_, p, q)| near(*p, at([0.0, 0.0]), 0.05) && near(*q, at([10.0, 0.0]), 0.05))
        .expect("the wall line");
    assert_eq!(wall.0.stroke, [0.0, 0.0, 0.0]);
    assert!(close(wall.0.width, 1.4173, 0.0001), "{}", wall.0.width);
    assert!(page.ops.contains(&"S".to_string()));
    // Every stroke is black at 0.5 mm: no red, no green, nothing else.
    for (paint, _, _) in &strokes {
        assert_eq!(paint.stroke, [0.0, 0.0, 0.0], "{paint:?}");
        assert!(close(paint.width, 1.4173, 0.0001), "{paint:?}");
    }
    // The doors (green, inside blocks on A-DOOR) are not on the page at all.
    for door in [
        ([1.0, 1.0], [1.9, 1.0]),
        ([5.0, 1.0], [5.0, 1.9]),
        ([9.0, 1.0], [10.8, 1.0]),
    ] {
        let (a, b) = (at(door.0), at(door.1));
        assert!(
            !strokes
                .iter()
                .any(|(_, p, q)| (near(*p, a, 0.5) && near(*q, b, 0.5))
                    || (near(*p, b, 0.5) && near(*q, a, 0.5))),
            "a door line at {door:?}"
        );
    }
    assert!(page.paints.iter().all(|p| p.fill != [0.0, 1.0, 0.0]));
    assert!(page.glyphs.iter().all(|g| g.fill != [0.0, 1.0, 0.0]));
    // The title (colour 7, white) is black text on white paper.
    assert!(!page.glyphs.is_empty());
    assert!(page.glyphs.iter().all(|g| g.fill == [0.0, 0.0, 0.0]));
}

/// Without a plot style, entities plot at their own lineweight (0.35 mm here), the rest at the
/// default 0.25 mm; with lineweights off, everything is a hairline.
#[test]
fn lineweights_plot_at_their_width_in_millimetres() {
    let (dir, mut s) = sample();
    let wall = s
        .drawing
        .doc
        .entities_in_block(MODEL)
        .find(|e| matches!(e, EntityType::Line(_)) && e.common().layer == "A-WALL")
        .map(|e| e.common().handle.value())
        .unwrap();
    s.run(Command::SetProperty {
        target: t(wall),
        prop: EntityProp::Lineweight,
        value: json!(35),
    })
    .unwrap();
    let mut j = job(MODEL);
    j.area = Area::Window([[-5.0, -5.0], [25.0, 5.0]]);
    j.scale = Scale::Ratio(1.0, 100.0);
    j.centre = false;
    let out = plot_pdf_to(&s, &j, &dir.path().join("lw.pdf")).unwrap();
    let page = read(&out.path);
    let at = |p: [f64; 2]| page_of(p, [-5.0, -5.0], 10.0);
    let strokes = page.strokes();
    let line = strokes
        .iter()
        .find(|(_, p, q)| near(*p, at([0.0, 0.0]), 0.05) && near(*q, at([10.0, 0.0]), 0.05))
        .unwrap();
    assert!(
        close(line.0.width, 0.35 * PT_PER_MM, 0.0001),
        "{}",
        line.0.width
    );
    let arc = strokes
        .iter()
        .find(|(_, p, _)| near(*p, at([22.0, 0.0]), 0.5))
        .expect("the arc");
    assert!(
        close(arc.0.width, 0.25 * PT_PER_MM, 0.0001),
        "{}",
        arc.0.width
    );

    j.lineweights = false;
    let out = plot_pdf_to(&s, &j, &dir.path().join("hair.pdf")).unwrap();
    let page = read(&out.path);
    assert!(page.strokes().iter().all(|(p, _, _)| p.width == 0.0));
}

/// A layer set not to plot is left out; the others plot.
#[test]
fn a_layer_that_does_not_plot_is_left_out() {
    let (dir, mut s) = sample();
    s.drawing.doc.layers.get_mut("A-WALL").unwrap().is_plottable = false;
    let mut j = job(MODEL);
    j.area = Area::Window([[-5.0, -5.0], [25.0, 5.0]]);
    j.scale = Scale::Ratio(1.0, 100.0);
    j.centre = false;
    let out = plot_pdf_to(&s, &j, &dir.path().join("noplot.pdf")).unwrap();
    let page = read(&out.path);
    let strokes = page.strokes();
    assert!(strokes.iter().all(|(p, _, _)| p.stroke != [1.0, 0.0, 0.0]));
    assert!(strokes.iter().any(|(p, _, _)| p.stroke == [0.0, 1.0, 0.0]));
}

/// Fit to paper centres the extents on the page, inside a 5 mm margin.
#[test]
fn fit_centres_the_extents_on_the_page() {
    let dir = tempfile::tempdir().unwrap();
    let mut doc = CadDocument::new();
    doc.header.insertion_units = 4;
    let mut ring = LwPolyline::new();
    for (x, y) in [
        (100.0, 200.0),
        (130.0, 200.0),
        (130.0, 220.0),
        (100.0, 220.0),
    ] {
        ring.add_point(Vector2::new(x, y));
    }
    ring.is_closed = true;
    doc.add_entity(EntityType::LwPolyline(ring)).unwrap();
    let path = write_drawing(&doc, &dir.path().join("box.dxf"));
    let s = Session::open(&path).unwrap();
    let mut j = job(MODEL);
    j.paper = paper("ISO A4");
    let out = plot_pdf(&s, &j).unwrap();
    assert_eq!(out.path.file_name().unwrap(), "box-Model.pdf");
    let page = read(&out.path);
    let pts: Vec<[f32; 2]> = page
        .strokes()
        .iter()
        .flat_map(|(_, p, q)| [*p, *q])
        .collect();
    let lo = pts
        .iter()
        .fold([f32::MAX; 2], |a, p| [a[0].min(p[0]), a[1].min(p[1])]);
    let hi = pts
        .iter()
        .fold([f32::MIN; 2], |a, p| [a[0].max(p[0]), a[1].max(p[1])]);
    let (w, h) = (297.0 * PT_PER_MM, 210.0 * PT_PER_MM);
    assert!(close((lo[0] + hi[0]) / 2.0, w / 2.0, 0.5), "{lo:?} {hi:?}");
    assert!(close((lo[1] + hi[1]) / 2.0, h / 2.0, 0.5), "{lo:?} {hi:?}");
    // 30 x 20 fits 287 mm wide (the height has room to spare).
    assert!(
        close(hi[0] - lo[0], 287.0 * PT_PER_MM, 0.5),
        "{lo:?} {hi:?}"
    );
}

/// A solid hatch plots as a filled path in its layer's colour.
#[test]
fn a_solid_hatch_plots_as_a_filled_path_in_its_colour() {
    let (dir, mut s) = sample();
    s.current.layer = "A-WALL".to_string();
    s.run(Command::Draw(DrawSpec::Hatch {
        boundary_handles: Vec::new(),
        points: vec![[30.0, 0.0], [34.0, 0.0], [34.0, 3.0], [30.0, 3.0]].into(),
        holes: Vec::new(),
        bulges: Vec::new(),
        pattern: HatchPattern::Solid,
    }))
    .unwrap();
    let mut j = job(MODEL);
    j.area = Area::Window([[-5.0, -5.0], [35.0, 5.0]]);
    j.scale = Scale::Ratio(1.0, 100.0);
    j.centre = false;
    let out = plot_pdf_to(&s, &j, &dir.path().join("fill.pdf")).unwrap();
    let page = read(&out.path);
    let fills: Vec<_> = page
        .paints
        .iter()
        .filter(|p| matches!(p.op.as_str(), "f" | "B" | "f*" | "B*") && p.fill == [1.0, 0.0, 0.0])
        .collect();
    assert!(!fills.is_empty(), "a red fill");
    let corner = page_of([34.0, 3.0], [-5.0, -5.0], 10.0);
    assert!(fills
        .iter()
        .flat_map(|p| p.subpaths.iter().flatten())
        .any(|p| near(*p, corner, 0.05)));
}

/// Arabic text is real PDF text: an embedded TrueType subset (FontFile2) whose ToUnicode maps
/// its glyphs back to the Arabic letters, shown with Tj.
#[test]
fn arabic_text_is_real_text_with_an_embedded_subset_and_a_to_unicode_map() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_drawing(&arabic_text_doc(), &dir.path().join("arabic.dxf"));
    let s = Session::open(&path).unwrap();
    let out = plot_pdf(&s, &job(MODEL)).unwrap();
    let page = read(&out.path);
    assert!(page.ops.contains(&"BT".to_string()));
    assert!(page.ops.contains(&"Tj".to_string()));
    assert!(!page.glyphs.is_empty());
    let mut fonts: Vec<String> = page.glyphs.iter().map(|g| g.font.clone()).collect();
    fonts.sort();
    fonts.dedup();
    let mut unicode = String::new();
    for name in &fonts {
        let font = page.font(name);
        assert_eq!(font.get(b"Subtype").unwrap().as_name().unwrap(), b"Type0");
        assert_eq!(
            font.get(b"Encoding").unwrap().as_name().unwrap(),
            b"Identity-H"
        );
        let cid = page.deref(font, b"DescendantFonts");
        assert_eq!(
            cid.get(b"Subtype").unwrap().as_name().unwrap(),
            b"CIDFontType2"
        );
        let descriptor = page.deref(cid, b"FontDescriptor");
        let file = page.stream(descriptor, b"FontFile2");
        assert!(
            ttf_parser::Face::parse(&file, 0).is_ok(),
            "an embedded TrueType"
        );
        let base = font.get(b"BaseFont").unwrap().as_name().unwrap();
        assert_eq!(
            base[6],
            b'+',
            "a subset tag: {}",
            String::from_utf8_lossy(base)
        );
        let cmap = String::from_utf8(page.stream(font, b"ToUnicode")).unwrap();
        assert!(cmap.contains("beginbfchar"));
        for line in cmap
            .lines()
            .filter(|l| l.starts_with('<') && l.contains("> <"))
        {
            let hex = line.split("> <").nth(1).unwrap().trim_end_matches('>');
            let units: Vec<u16> = (0..hex.len())
                .step_by(4)
                .map(|i| u16::from_str_radix(&hex[i..i + 4], 16).unwrap())
                .collect();
            unicode.push_str(&String::from_utf16(&units).unwrap());
        }
    }
    for letter in ['غ', 'ر', 'ف', 'ة', 'م', 'ط', 'ب', 'خ', 'D', 'o'] {
        assert!(unicode.contains(letter), "{letter} in {unicode}");
    }
}

/// Noto Sans Arabic draws جميل and حميل with the same glyphs bar a dot mark, so the ToUnicode
/// map alone gives both back as one word. The glyph whose map entry is another letter's is
/// shown inside a span whose `ActualText` is its own letter; pdftotext (when the computer has
/// it) gives every word back.
#[test]
fn arabic_words_whose_glyphs_collide_copy_back_as_themselves() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_drawing(&arabic_text_doc(), &dir.path().join("arabic.dxf"));
    let mut s = Session::open(&path).unwrap();
    let words = ["جميل", "حميل", "بنت", "نبت", "الهندسي", "سلام"];
    for (i, word) in words.iter().enumerate() {
        s.run(Command::Text {
            at: [1.0, 6.0 + 2.0 * i as f64],
            height: 0.5,
            rotation: 0.0,
            value: word.to_string(),
            style: None,
        })
        .unwrap();
    }
    let out = plot_pdf(&s, &job(MODEL)).unwrap();
    let doc = lopdf::Document::load(&out.path).unwrap();
    let page = *doc.get_pages().values().next().unwrap();
    let content = lopdf::content::Content::decode(&doc.get_page_content(page)).unwrap();
    let mut spans = Vec::new();
    let mut open = 0;
    for op in &content.operations {
        match op.operator.as_str() {
            "BDC" => {
                open += 1;
                let props = op.operands[1].as_dict().unwrap();
                let bytes = props.get(b"ActualText").unwrap().as_str().unwrap();
                assert_eq!(&bytes[..2], &[0xFE, 0xFF], "UTF-16BE with its mark");
                let units: Vec<u16> = bytes[2..]
                    .chunks(2)
                    .map(|c| u16::from_be_bytes([c[0], c[1]]))
                    .collect();
                spans.push(String::from_utf16(&units).unwrap());
            }
            "EMC" => open -= 1,
            "Tj" => assert!(open <= 1),
            _ => {}
        }
    }
    assert_eq!(open, 0);
    // جميل is drawn first and so owns the shared hah body in the map; حميل's needs its span.
    assert!(spans.iter().any(|t| t == "ح"), "ح in {spans:?}");
    match std::process::Command::new("pdftotext")
        .arg(&out.path)
        .arg("-")
        .output()
    {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout);
            for word in words {
                assert!(text.contains(word), "{word} in pdftotext's {text}");
            }
        }
        _ => eprintln!("pdftotext is not on this computer: the extraction check is skipped"),
    }
}

/// The output goes beside the drawing by default, and only inside the drawing's folder by
/// command; a missing sheet or an empty area is an error.
#[test]
fn output_goes_beside_the_drawing_and_never_outside_its_folder() {
    let (dir, s) = sample();
    let out = plot_pdf(&s, &job(MODEL)).unwrap();
    let folder = dir.path().canonicalize().unwrap();
    assert_eq!(out.path, folder.join("sample-Model.pdf"));
    assert!(out.path.exists());

    std::fs::create_dir(dir.path().join("plots")).unwrap();
    let mut j = job(MODEL);
    j.output = Some("plots/a.pdf".to_string());
    assert_eq!(
        plot_pdf(&s, &j).unwrap().path,
        folder.join("plots").join("a.pdf")
    );

    for bad in [
        "../escape.pdf",
        "/tmp/elsewhere.pdf",
        "plots/a.txt",
        "nofolder/a.pdf",
    ] {
        j.output = Some(bad.to_string());
        assert!(
            matches!(plot_pdf(&s, &j), Err(PlotError::Output(_))),
            "{bad} refused"
        );
    }
    assert!(!dir.path().parent().unwrap().join("escape.pdf").exists());

    let mut j = job("*Paper_Nowhere");
    j.output = None;
    assert!(matches!(plot_pdf(&s, &j), Err(PlotError::NoSuchSheet(_))));
    let mut j = job(MODEL);
    j.area = Area::Window([[1.0, 1.0], [1.0, 1.0]]);
    assert!(matches!(plot_pdf(&s, &j), Err(PlotError::Nothing)));
}

#[test]
fn the_paper_catalog_has_iso_ansi_and_arch_sizes_in_portrait() {
    let papers = paper_catalog();
    let a1 = paper("ISO A1");
    assert_eq!((a1.w_mm, a1.h_mm), (594.0, 841.0));
    let d = paper("ANSI D");
    assert_eq!((d.w_mm, d.h_mm), (558.8, 863.6));
    assert!(papers.iter().any(|p| p.name == "ARCH E1"));
    assert!(papers.iter().all(|p| p.w_mm <= p.h_mm));
}

#[test]
fn a_job_reads_and_writes_its_wire_form() {
    let wire = json!({
        "sheet": "*Model_Space",
        "area": {"window": [[0, 0], [10, 5]]},
        "paper": {"name": "ISO A3", "w_mm": 297, "h_mm": 420},
        "orientation": "landscape",
        "scale": {"ratio": [1, 100]},
        "centre": true,
        "style": null,
        "lineweights": true,
        "output": "plan.pdf"
    });
    let j: PlotJob = serde_json::from_value(wire).unwrap();
    assert_eq!(j.area, Area::Window([[0.0, 0.0], [10.0, 5.0]]));
    assert_eq!(j.scale, Scale::Ratio(1.0, 100.0));
    assert_eq!(j.orientation, Orientation::Landscape);
    let back = serde_json::to_value(&j).unwrap();
    assert_eq!(back["area"], json!({"window": [[0.0, 0.0], [10.0, 5.0]]}));
    let extents: Area = serde_json::from_value(json!("extents")).unwrap();
    assert_eq!(extents, Area::Extents);
    let fit: Scale = serde_json::from_value(json!("fit")).unwrap();
    assert_eq!(fit, Scale::Fit);
}

/// Review Focus 2 again, from a CTB file on disk (written in AutoCAD's layout, its text ending
/// in a NUL): the walls plot black at 0.5 mm, nothing green.
#[test]
fn review_focus_2_from_a_ctb_file() {
    let (dir, s) = sample();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/office.ctb");
    let mut j = job(MODEL);
    j.area = Area::Window([[-5.0, -5.0], [25.0, 5.0]]);
    j.scale = Scale::Ratio(1.0, 100.0);
    j.centre = false;
    j.style = Some(PlotStyle::load(&path).unwrap());
    let out = plot_pdf_to(&s, &j, &dir.path().join("file-ctb.pdf")).unwrap();
    let page = read(&out.path);
    let at = |p: [f64; 2]| page_of(p, [-5.0, -5.0], 10.0);
    let strokes = page.strokes();
    assert!(strokes
        .iter()
        .any(|(paint, p, q)| near(*p, at([0.0, 0.0]), 0.05)
            && near(*q, at([10.0, 0.0]), 0.05)
            && paint.stroke == [0.0, 0.0, 0.0]
            && close(paint.width, 1.4173, 0.0001)));
    assert!(strokes.iter().all(|(p, _, _)| p.stroke != [0.0, 1.0, 0.0]));
    assert!(!strokes
        .iter()
        .any(|(_, p, q)| near(*p, at([1.0, 1.0]), 0.5) && near(*q, at([1.9, 1.0]), 0.5)));
}

/// Curves are flattened for the paper, not the screen: a 1 m radius at 1:100 (10 mm on paper)
/// has at least 16 segments a quarter turn, each within 0.02 mm of the circle.
#[test]
fn a_1_m_arc_at_1_to_100_has_16_segments_a_quarter() {
    let (dir, mut s) = sample();
    s.run(Command::Draw(DrawSpec::Circle {
        centre: [30.0, 0.0],
        radius: 1.0,
    }))
    .unwrap();
    let mut j = job(MODEL);
    j.area = Area::Window([[25.0, -5.0], [35.0, 5.0]]);
    j.scale = Scale::Ratio(1.0, 100.0);
    j.centre = false;
    let out = plot_pdf_to(&s, &j, &dir.path().join("arc.pdf")).unwrap();
    let page = read(&out.path);
    let c = page_of([30.0, 0.0], [25.0, -5.0], 10.0);
    let r = 10.0 * PT_PER_MM;
    let on = |p: [f32; 2]| close((p[0] - c[0]).hypot(p[1] - c[1]), r, 0.05);
    let circle: Vec<_> = page
        .strokes()
        .into_iter()
        .filter(|(_, p, q)| on(*p) && on(*q))
        .collect();
    let quarter = circle
        .iter()
        .filter(|(_, p, q)| {
            [p, q]
                .iter()
                .all(|v| v[0] >= c[0] - 0.01 && v[1] >= c[1] - 0.01)
        })
        .count();
    assert!(quarter >= 16, "{quarter} segments a quarter turn");
    for (_, p, q) in &circle {
        let m = [(p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0];
        let off = r - (m[0] - c[0]).hypot(m[1] - c[1]);
        assert!(off <= 0.02 * PT_PER_MM, "{off} pt off the circle");
    }
}

/// A layout whose page setup is in inches: one layout unit is 25.4 mm on the paper at 1:1, and
/// "layout" is its paper with the origin at the printable area's lower left.
#[test]
fn an_inch_layout_plots_one_unit_as_an_inch() {
    let (dir, mut s) = sample();
    let layout = s
        .drawing
        .sheets()
        .into_iter()
        .find(|sh| !sh.is_model)
        .unwrap();
    for o in s.drawing.doc.objects.values_mut() {
        if let opencadcodec::objects::ObjectType::Layout(l) = o {
            if l.name == layout.name {
                l.paper_width = 279.4;
                l.paper_height = 215.9;
                l.plot_paper_units = 0;
                l.plot_margin_left = 6.35;
                l.plot_margin_bottom = 6.35;
            }
        }
    }
    s.set_sheet(&layout.block).unwrap();
    s.run(Command::Draw(DrawSpec::Line {
        a: [1.0, 1.0],
        b: [2.0, 1.0],
    }))
    .unwrap();
    let mut j = job(&layout.block);
    j.area = Area::Layout;
    j.paper = paper("ANSI A");
    j.scale = Scale::Ratio(1.0, 1.0);
    j.centre = false;
    let out = plot_pdf_to(&s, &j, &dir.path().join("inch.pdf")).unwrap();
    let page = read(&out.path);
    let inch = 25.4 * PT_PER_MM;
    let margin = 6.35 * PT_PER_MM;
    let (a, b) = (
        [inch + margin, inch + margin],
        [2.0 * inch + margin, inch + margin],
    );
    assert!(
        page.strokes()
            .iter()
            .any(|(_, p, q)| near(*p, a, 0.05) && near(*q, b, 0.05)),
        "a line from {a:?} to {b:?}"
    );
}

/// Arabic stored in visual order (the drawing's "Arabic text is reversed") is not its reading
/// text: it plots as outlines, never as PDF text with a wrong ToUnicode map. Text Qiyas wrote
/// (marked logical) stays real text.
#[test]
fn reversed_arabic_plots_as_outlines_and_logical_text_stays_text() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_drawing(&arabic_text_doc(), &dir.path().join("arabic.dxf"));
    let mut s = Session::open(&path).unwrap();
    s.arabic_reversed = true;
    s.run(Command::Text {
        at: [1.0, 6.0],
        height: 0.5,
        rotation: 0.0,
        value: "مرحبا".to_string(),
        style: None,
    })
    .unwrap();
    let out = plot_pdf(&s, &job(MODEL)).unwrap();
    let page = read(&out.path);
    let mut unicode = String::new();
    let mut fonts: Vec<String> = page.glyphs.iter().map(|g| g.font.clone()).collect();
    fonts.sort();
    fonts.dedup();
    for name in &fonts {
        let font = page.font(name);
        let cmap = String::from_utf8(page.stream(font, b"ToUnicode")).unwrap();
        for line in cmap
            .lines()
            .filter(|l| l.starts_with('<') && l.contains("> <"))
        {
            let hex = line.split("> <").nth(1).unwrap().trim_end_matches('>');
            let units: Vec<u16> = (0..hex.len())
                .step_by(4)
                .map(|i| u16::from_str_radix(&hex[i..i + 4], 16).unwrap())
                .collect();
            unicode.push_str(&String::from_utf16(&units).unwrap());
        }
    }
    for letter in ['ر', 'ح', 'ب', 'ا'] {
        assert!(unicode.contains(letter), "{letter} in {unicode}");
    }
    for letter in ['خ', 'ط', 'غ', 'ش', 'ع'] {
        assert!(!unicode.contains(letter), "{letter} in {unicode}");
    }
    // The reversed texts (red, on A-WALL) are filled outlines.
    assert!(page
        .paints
        .iter()
        .any(|p| p.op == "f" && p.fill == [1.0, 0.0, 0.0]));
}

/// Defpoints never plots, whatever its plot flag: a viewport frame on it is not on the paper,
/// and a model line on it neither; the rest plots.
#[test]
fn defpoints_never_plots() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::open(&write_sample(dir.path(), "dwg")).unwrap();
    s.run(Command::NewLayer {
        name: "Defpoints".to_string(),
    })
    .unwrap();
    let wall = s
        .drawing
        .doc
        .entities_in_block(MODEL)
        .find(|e| matches!(e, EntityType::Line(_)) && e.common().layer == "A-WALL")
        .map(|e| e.common().handle.value())
        .unwrap();
    s.run(Command::SetProperty {
        target: t(wall),
        prop: EntityProp::Layer,
        value: json!("Defpoints"),
    })
    .unwrap();
    let layout = s
        .drawing
        .sheets()
        .into_iter()
        .find(|sh| !sh.is_model)
        .unwrap();
    s.set_sheet(&layout.block).unwrap();
    s.current.layer = "Defpoints".to_string();
    s.run(Command::MView {
        model_bounds: None,
        paper_centre: None,
        target: None,
        scale: None,
        centre: None,
        corner1: [20.0, 20.0],
        corner2: [820.0, 570.0],
    })
    .unwrap();
    let mut j = job(&layout.block);
    j.paper = paper("ISO A1");
    j.scale = Scale::Ratio(1.0, 1.0);
    let out = plot_pdf_to(&s, &j, &dir.path().join("defpoints.pdf")).unwrap();
    let page = read(&out.path);
    let strokes = page.strokes();
    assert!(!strokes.iter().any(|(_, p, q)| {
        close((q[0] - p[0]).abs(), 800.0 * PT_PER_MM, 0.05) && close(p[1], q[1], 0.01)
    }));
    assert!(
        strokes.iter().all(|(p, _, _)| p.clipped),
        "only model space inside the frame"
    );
    assert!(
        strokes.iter().any(|(p, _, _)| p.stroke == [1.0, 0.0, 0.0]),
        "the other walls"
    );
    assert!(
        strokes.iter().any(|(p, _, _)| p.stroke == [0.0, 1.0, 0.0]),
        "the doors"
    );
    // The 10 m wall (now on Defpoints) is not there: no stroke 10 m long at the viewport's scale.
    let view = s.paper_views()[0].clone();
    let ten_m = (10.0 * view.scale) as f32 * PT_PER_MM;
    assert!(!strokes
        .iter()
        .any(|(_, p, q)| close((q[0] - p[0]).hypot(q[1] - p[1]), ten_m, 0.05)));
}

/// Pictures plot inside viewports too (clipped to the frame), at up to 300 dpi; a picture whose
/// file is gone is left out and `skipped` says which and why.
#[test]
fn a_viewport_plots_the_picture_under_it_and_a_missing_one_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Session::open(&write_sample(dir.path(), "dwg")).unwrap();
    let png = qiyas_core::fixtures::write_png(dir.path(), "pic.png");
    s.attach_image(&png, [5.0, 5.0], 20.0).unwrap();
    let layout = s
        .drawing
        .sheets()
        .into_iter()
        .find(|sh| !sh.is_model)
        .unwrap();
    s.set_sheet(&layout.block).unwrap();
    let Outcome::Added { handles } = s
        .run(Command::MView {
            model_bounds: None,
            paper_centre: None,
            target: None,
            scale: None,
            centre: None,
            corner1: [20.0, 20.0],
            corner2: [820.0, 570.0],
        })
        .unwrap()
    else {
        panic!("a viewport");
    };
    s.run(Command::SetProperty {
        target: t(handles[0]),
        prop: EntityProp::ViewportScale,
        value: json!(10.0),
    })
    .unwrap();
    let mut j = job(&layout.block);
    j.paper = paper("ISO A1");
    j.scale = Scale::Ratio(1.0, 1.0);
    let out = plot_pdf_to(&s, &j, &dir.path().join("pic.pdf")).unwrap();
    assert!(out.skipped.is_empty(), "{:?}", out.skipped);
    let page = read(&out.path);
    assert_eq!(page.pictures.len(), 1);
    let (m, clipped) = page.pictures[0];
    assert!(clipped, "inside the viewport's clip");
    // 20 m wide at 1:100: 200 mm on the paper.
    assert!(close(m[0].hypot(m[1]), 200.0 * PT_PER_MM, 0.05), "{m:?}");

    std::fs::remove_file(&png).unwrap();
    let out = plot_pdf_to(&s, &j, &dir.path().join("nopic.pdf")).unwrap();
    assert_eq!(read(&out.path).pictures.len(), 0);
    assert!(
        out.skipped.iter().any(|w| w.contains("pic.png")),
        "{:?}",
        out.skipped
    );
}

/// The session is needed only to take the snapshot: the PDF is written from it while another
/// thread holds the session.
#[test]
fn the_pdf_is_written_from_a_snapshot_without_the_session() {
    fn send_and_static<T: Send + 'static>(_: &T) {}
    let (dir, s) = sample();
    let mut j = job(MODEL);
    j.area = Area::Window([[-5.0, -5.0], [25.0, 5.0]]);
    j.scale = Scale::Ratio(1.0, 100.0);
    j.centre = false;
    let locked = std::sync::Arc::new(std::sync::Mutex::new(s));
    let snapshot = {
        let session = locked.lock().unwrap();
        plot_snapshot(&session, &j).unwrap()
    };
    send_and_static(&snapshot);
    // Another thread holds the session the whole time the PDF is written.
    let held = std::sync::Arc::clone(&locked);
    let (tx, rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
    let holder = std::thread::spawn(move || {
        let _session = held.lock().unwrap();
        tx.send(()).unwrap();
        // Holds it until the PDF is written.
        done_rx.recv().unwrap();
    });
    rx.recv().unwrap();
    let path = dir.path().join("snapshot.pdf");
    let out = write_pdf(&snapshot, &path).unwrap();
    assert!(locked.try_lock().is_err(), "still held while writing");
    done_tx.send(()).unwrap();
    holder.join().unwrap();
    let page = read(&out.path);
    let at = |p: [f64; 2]| page_of(p, [-5.0, -5.0], 10.0);
    assert!(page
        .strokes()
        .iter()
        .any(|(_, p, q)| near(*p, at([0.0, 0.0]), 0.05) && near(*q, at([10.0, 0.0]), 0.05)));
    assert!(
        !page.glyphs.is_empty(),
        "the title as text, its font from the snapshot"
    );
}

#[test]
fn plotted_linetypes_and_hidden_points_match_the_screen_in_every_unit() {
    for units in [4, 5, 6, 1] {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Session::new_blank(&dir.path().join("patterns.dxf"), units);
        let k = 1.0 / s.drawing.units.to_metres();
        s.run(Command::AnnotationScale {
            name: "1:50".into(),
        })
        .unwrap();
        s.run(Command::SymmetryAxis {
            p1: [0.0, 0.0],
            p2: [10.0 * k, 0.0],
        })
        .unwrap();
        s.drawing.doc.header.point_display_mode = 1;
        s.drawing
            .doc
            .add_entity(EntityType::Point(opencadcodec::entities::Point::at(
                opencadcodec::Vector3::new(5.0 * k, k, 0.0),
            )))
            .unwrap();
        let path = dir.path().join("patterns.pdf");
        plot_pdf_to(&s, &job(MODEL), &path).unwrap();
        if let Some(folder) = std::env::var_os("QIYAS_GOAL_G_ARTIFACTS") {
            let folder = std::path::Path::new(&folder);
            std::fs::create_dir_all(folder).unwrap();
            std::fs::copy(&path, folder.join(format!("patterns-units-{units}.pdf"))).unwrap();
        }
        let page = read(&path);
        let segments: Vec<_> = page
            .paints
            .iter()
            .flat_map(|p| &p.subpaths)
            .filter(|p| p.len() == 2)
            .collect();
        assert!(
            segments.len() > 10,
            "CENTER must draw dashes: {}",
            segments.len()
        );
        assert!(
            segments.iter().all(|p| (p[1][1] - p[0][1]).abs() < 0.01),
            "Hidden points must add no crosses"
        );
    }
}
