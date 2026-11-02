//! Plot styles: CTB and STB files (zlib-compressed text) read, edited, written and applied.

use std::io::Write;

use proptest::prelude::*;
use qiyas_plot::{Pen, PlotStyle, StyleKind};

proptest! {
    #[test]
    fn ctb_parse_never_panics_and_reaches_a_fixed_point(body in "[a-z_0-9={}\"\n. -]{0,400}") {
        if let Ok(style) = PlotStyle::parse(format!("description=\"x\n{body}").as_bytes()) {
            let bytes = style.to_bytes();
            prop_assert_eq!(PlotStyle::parse(&bytes).unwrap().to_bytes(), bytes);
        }
    }

    #[test]
    fn ctb_parse_never_panics_on_bytes(bytes in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let _ = PlotStyle::parse(&bytes);
    }
}

/// A small CTB text as AutoCAD writes it (three colours shown; the rest are the defaults), with
/// a key Qiyas does not know.
fn ctb_text() -> String {
    let mut t = String::from(
        "description=\"Office pens\r\naci_table_available=TRUE\r\nscale_factor=1.0\r\n\
         apply_factor=FALSE\r\ncustom_lineweight_display_units=0\r\nfuture_key=42\r\naci_table{\r\n",
    );
    for i in 0..255 {
        t.push_str(&format!(" {i}=\"Color_{}\r\n", i + 1));
    }
    t.push_str("}\r\nplot_style{\r\n");
    for i in 0..255 {
        let (color, lw, screen) = match i {
            0 => ("-1040187392", 13, 100),
            2 => ("-1006632961", 0, 0),
            4 => ("-1040187136", 8, 50),
            _ => ("-1006632961", 0, 100),
        };
        t.push_str(&format!(
            " {i}{{\r\n  name=\"Color_{n}\r\n  localized_name=\"Color_{n}\r\n  description=\"\r\n  \
             color={color}\r\n  mode_color={color}\r\n  color_policy=1\r\n  physical_pen_number=0\r\n  \
             virtual_pen_number=0\r\n  screen={screen}\r\n  linepattern_size=0.5\r\n  linetype=31\r\n  \
             adaptive_linetype=TRUE\r\n  lineweight={lw}\r\n  fill_style=73\r\n  end_style=4\r\n  \
             join_style=5\r\n  pen_extra=\"kept\r\n }}\r\n",
            n = i + 1
        ));
    }
    t.push_str("}\r\ncustom_lineweight_table{\r\n");
    let weights = [
        0.0, 0.05, 0.09, 0.1, 0.13, 0.15, 0.18, 0.2, 0.25, 0.3, 0.35, 0.4, 0.45, 0.5, 0.53, 0.6,
        0.65, 0.7, 0.8, 0.9, 1.0, 1.06, 1.2, 1.4, 1.58, 2.0, 2.11,
    ];
    for (i, w) in weights.iter().enumerate() {
        t.push_str(&format!(" {i}={w:.2}\r\n"));
    }
    t.push_str("}\r\n");
    t
}

/// `text` in the binary form: the 48-byte header, adler32 of the zlib body, the text's length,
/// the body's length, then the body.
fn binary(header: &str, text: &str) -> Vec<u8> {
    binary_bytes(header, text.as_bytes())
}

/// `binary` over raw text bytes (not UTF-8, or with a NUL at the end as AutoCAD writes it).
fn binary_bytes(header: &str, text: &[u8]) -> Vec<u8> {
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(text).unwrap();
    let body = z.finish().unwrap();
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for byte in &body {
        a = (a + u32::from(*byte)) % 65521;
        b = (b + a) % 65521;
    }
    let mut out = header.as_bytes().to_vec();
    assert_eq!(out.len(), 48);
    out.extend((b << 16 | a).to_le_bytes());
    out.extend((text.len() as u32).to_le_bytes());
    out.extend((body.len() as u32).to_le_bytes());
    out.extend(body);
    out
}

/// The text inside a compressed plot style file, and the text length its header gives.
fn unpacked(bytes: &[u8]) -> (Vec<u8>, usize) {
    let mut text = Vec::new();
    std::io::Read::read_to_end(&mut flate2::read::ZlibDecoder::new(&bytes[60..]), &mut text)
        .unwrap();
    let len = u32::from_le_bytes(bytes[52..56].try_into().unwrap()) as usize;
    (text, len)
}

/// Where the checked-in fixtures live (written by Qiyas's own writer: `write_fixtures`).
fn fixture(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// The fixtures' tables: the office CTB (pen 1 black at 0.50 mm, pen 3 not plotted, pen 5
/// screened to half), and an STB with Normal and a heavy black style.
fn office_ctb() -> PlotStyle {
    let mut ctb = PlotStyle::default_ctb();
    ctb.description = "Qiyas test pens".to_string();
    ctb.styles[0].color = Some([0, 0, 0]);
    ctb.styles[0].lineweight = 13;
    ctb.styles[2].screen = 0;
    ctb.styles[4].screen = 50;
    ctb
}

fn named_stb() -> PlotStyle {
    let office = office_ctb();
    let named = |i: usize, name: &str| {
        let mut style = office.styles[i].clone();
        style.name = name.to_string();
        style.localized_name = name.to_string();
        style
    };
    PlotStyle {
        kind: StyleKind::Stb,
        description: "Qiyas test styles".to_string(),
        styles: vec![named(1, "Normal"), named(0, "Heavy")],
        ..office.clone()
    }
}

/// Writes the fixtures again: `cargo test -p qiyas-plot --test ctb -- --ignored write_fixtures`.
#[test]
#[ignore]
fn write_fixtures() {
    std::fs::create_dir_all(fixture("")).unwrap();
    office_ctb().save(&fixture("office.ctb")).unwrap();
    named_stb().save(&fixture("named.stb")).unwrap();
}

const CTB_HEADER: &str = "PIAFILEVERSION_2.0,CTBVER1,compress\r\npmzlibcodec";
const STB_HEADER: &str = "PIAFILEVERSION_2.0,STBVER1,compress\r\npmzlibcodec";

#[test]
fn deep_nesting_is_refused_not_a_crash() {
    let text = format!("description=\"x\"\n{}", "a{\n".repeat(200_000));
    let result = PlotStyle::parse(text.as_bytes());
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("nested too deeply"));
}

#[test]
fn compressed_deep_nesting_is_refused_not_a_crash() {
    let text = format!("description=\"x\"\n{}", "a{\n".repeat(200_000));
    let result = PlotStyle::parse(&binary(CTB_HEADER, &text));
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("nested too deeply"));
}

#[test]
fn nesting_at_the_limit_still_parses() {
    let text = format!("{}{}{}", ctb_text(), "a{\n".repeat(8), "}\n".repeat(8));
    let expected = PlotStyle::parse(ctb_text().as_bytes()).unwrap();
    assert_eq!(PlotStyle::parse(text.as_bytes()).unwrap(), expected);
    assert_eq!(
        PlotStyle::parse(&binary(CTB_HEADER, &text)).unwrap(),
        expected
    );
}

#[test]
fn a_ctb_file_reads_its_pens() {
    let style = PlotStyle::parse(&binary(CTB_HEADER, &ctb_text())).unwrap();
    assert_eq!(style.kind, StyleKind::Ctb);
    assert_eq!(style.description, "Office pens");
    assert_eq!(style.styles.len(), 255);
    assert_eq!(style.lineweights.len(), 27);
    assert_eq!(style.styles[0].name, "Color_1");
    assert_eq!(style.styles[0].color, Some([0, 0, 0]));
    assert_eq!(style.styles[0].lineweight, 13);
    assert_eq!(style.styles[2].screen, 0);
    assert_eq!(style.styles[1].color, None, "object colour");
    // 0xC2000100: an explicit colour, RGB in the low 24 bits.
    assert_eq!(style.styles[4].color, Some([0, 1, 0]));
}

/// Review Focus 2's style: colour 1 black at 0.5 mm, colour 3 not plotted, the rest as drawn.
#[test]
fn applying_a_ctb_maps_colours_to_pens() {
    let style = PlotStyle::parse(&binary(CTB_HEADER, &ctb_text())).unwrap();
    assert_eq!(
        style.apply([255, 0, 0], None),
        Pen {
            rgb: [0, 0, 0],
            lineweight_mm: Some(0.5),
            plot: true
        }
    );
    assert!(
        !style.apply([0, 255, 0], None).plot,
        "colour 3 does not plot"
    );
    // Colour 2 (yellow) uses the object's colour and lineweight.
    assert_eq!(
        style.apply([255, 255, 0], None),
        Pen {
            rgb: [255, 255, 0],
            lineweight_mm: None,
            plot: true
        }
    );
    // Colour 5 (blue) screened to half: its pen colour (0, 1, 0) moves half way to white.
    let pen = style.apply([0, 0, 255], None);
    assert_eq!(pen.rgb, [128, 128, 128]);
    assert_eq!(pen.lineweight_mm, Some(0.25));
    // White (colour 7) plots black on white paper.
    assert_eq!(style.apply([255, 255, 255], None).rgb, [0, 0, 0]);
    // A true colour takes the nearest colour's pen: almost red is colour 1.
    assert_eq!(style.apply([250, 2, 3], None).rgb, [0, 0, 0]);
}

#[test]
fn a_ctb_round_trips_after_an_edit_and_keeps_what_it_does_not_know() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("office.ctb");
    std::fs::write(&source, binary(CTB_HEADER, &ctb_text())).unwrap();
    let mut style = PlotStyle::load(&source).unwrap();
    style.styles[6].color = Some([10, 20, 30]);
    style.styles[6].lineweight = 10;
    style.styles[6].screen = 80;
    style.description = "Edited".to_string();
    let saved = dir.path().join("edited.ctb");
    style.save(&saved).unwrap();
    let bytes = std::fs::read(&saved).unwrap();
    assert!(bytes.starts_with(CTB_HEADER.as_bytes()));
    let again = PlotStyle::load(&saved).unwrap();
    assert_eq!(again, style);
    assert_eq!(again.styles[6].color, Some([10, 20, 30]));
    assert_eq!(again.description, "Edited");
    // Unknown keys survive, at the top and in each pen.
    assert!(again
        .extra
        .iter()
        .any(|(k, v)| k == "future_key" && v == "42"));
    assert!(again.styles[0]
        .extra
        .iter()
        .any(|(k, v)| k == "pen_extra" && v == "\"kept"));
    // Saving again gives the same bytes.
    let twice = dir.path().join("twice.ctb");
    again.save(&twice).unwrap();
    assert_eq!(std::fs::read(&twice).unwrap(), bytes);
    // The header's checksum and lengths are right.
    let body = &bytes[60..];
    let len = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    assert_eq!(len(56), body.len());
    let mut z = flate2::read::ZlibDecoder::new(body);
    let mut text = String::new();
    std::io::Read::read_to_string(&mut z, &mut text).unwrap();
    assert_eq!(len(52), text.len());
}

#[test]
fn the_default_ctb_plots_everything_as_drawn() {
    let style = PlotStyle::default_ctb();
    assert_eq!(style.kind, StyleKind::Ctb);
    assert_eq!(style.styles.len(), 255);
    assert_eq!(style.lineweights[13], 0.5);
    let pen = style.apply([255, 0, 0], None);
    assert_eq!(
        pen,
        Pen {
            rgb: [255, 0, 0],
            lineweight_mm: None,
            plot: true
        }
    );
    // It writes and reads back as itself.
    let again = PlotStyle::parse(&style.to_bytes()).unwrap();
    assert_eq!(again, style);
}

#[test]
fn an_stb_applies_the_named_style_of_the_layer() {
    let text = "description=\"Named\naci_table_available=FALSE\nscale_factor=1.0\napply_factor=FALSE\n\
        custom_lineweight_display_units=0\nplot_style{\n 0{\n  name=\"Normal\n  localized_name=\"Normal\n  \
        description=\"\n  color=-1006632961\n  mode_color=-1006632961\n  color_policy=1\n  \
        physical_pen_number=0\n  virtual_pen_number=0\n  screen=100\n  linepattern_size=0.5\n  \
        linetype=31\n  adaptive_linetype=TRUE\n  lineweight=0\n  fill_style=73\n  end_style=4\n  \
        join_style=5\n }\n 1{\n  name=\"Heavy\n  localized_name=\"Heavy\n  description=\"\n  \
        color=-1040187392\n  mode_color=-1040187392\n  color_policy=3\n  physical_pen_number=0\n  \
        virtual_pen_number=0\n  screen=100\n  linepattern_size=0.5\n  linetype=31\n  \
        adaptive_linetype=TRUE\n  lineweight=20\n  fill_style=73\n  end_style=4\n  join_style=5\n }\n}\n\
        custom_lineweight_table{\n 0=0.00\n 20=1.00\n}\n";
    let style = PlotStyle::parse(&binary(STB_HEADER, text)).unwrap();
    assert_eq!(style.kind, StyleKind::Stb);
    assert_eq!(style.styles.len(), 2);
    let heavy = style.apply([255, 0, 0], Some("heavy"));
    assert_eq!(heavy.rgb, [0, 0, 0]);
    assert_eq!(heavy.lineweight_mm, Some(1.0));
    let normal = style.apply([255, 0, 0], None);
    assert_eq!(
        normal,
        Pen {
            rgb: [255, 0, 0],
            lineweight_mm: None,
            plot: true
        }
    );
    // An STB writes back as an STB.
    let bytes = style.to_bytes();
    assert!(bytes.starts_with(STB_HEADER.as_bytes()));
    assert_eq!(PlotStyle::parse(&bytes).unwrap(), style);
}

#[test]
fn grayscale_pens_plot_grey() {
    let mut style = PlotStyle::default_ctb();
    style.styles[0].color_policy = 2;
    let pen = style.apply([255, 0, 0], None);
    assert_eq!(pen.rgb[0], pen.rgb[1]);
    assert_eq!(pen.rgb[1], pen.rgb[2]);
    assert!(pen.rgb[0] > 0 && pen.rgb[0] < 255, "{:?}", pen.rgb);
}

#[test]
fn plain_text_is_read_too_and_junk_is_refused() {
    let style = PlotStyle::parse(ctb_text().as_bytes()).unwrap();
    assert_eq!(style.kind, StyleKind::Ctb);
    assert_eq!(style.styles[0].color, Some([0, 0, 0]));
    assert!(PlotStyle::parse(b"not a plot style").is_err());
    let mut bad = binary(CTB_HEADER, &ctb_text());
    bad.truncate(70);
    assert!(PlotStyle::parse(&bad).is_err());
    assert!(PlotStyle::load(std::path::Path::new("/nowhere/x.ctb")).is_err());
}

#[test]
fn a_plot_style_travels_as_json() {
    let style = PlotStyle::default_ctb();
    let wire = serde_json::to_value(&style).unwrap();
    assert_eq!(wire["kind"], "ctb");
    assert_eq!(wire["styles"][0]["name"], "Color_1");
    let back: PlotStyle = serde_json::from_value(wire).unwrap();
    assert_eq!(back, style);
}

/// AutoCAD's files end their text with a NUL that the header's length counts, use LF line ends,
/// and may hold a description in the computer's code page: such a file loads.
#[test]
fn a_file_shaped_like_autocads_loads() {
    let mut text = ctb_text().replace("\r\n", "\n").into_bytes();
    // "Café" in Windows-1252, not UTF-8.
    let at = text.iter().position(|b| *b == b'\n').unwrap();
    text.splice(at..at, [b' ', b'C', b'a', b'f', 0xE9]);
    text.push(0);
    let style = PlotStyle::parse(&binary_bytes(CTB_HEADER, &text)).unwrap();
    assert!(
        style.description.starts_with("Office pens Caf"),
        "{}",
        style.description
    );
    assert_eq!(style.styles[0].color, Some([0, 0, 0]));
    assert_eq!(style.styles[0].lineweight, 13);
    assert_eq!(style.styles[2].screen, 0);
    // Several NULs, and blank lines or spaces after the last brace, too.
    let mut padded = ctb_text().into_bytes();
    padded.extend(b"\r\n  \n\0\0");
    assert!(PlotStyle::parse(&binary_bytes(CTB_HEADER, &padded)).is_ok());
    // A plain-text table with the NUL as well.
    let mut plain = ctb_text().into_bytes();
    plain.push(0);
    assert!(PlotStyle::parse(&plain).is_ok());
}

/// Qiyas writes the file as AutoCAD does: LF lines, the text ending in a NUL the header's
/// length counts, the adler32 of the compressed body.
#[test]
fn the_writer_ends_the_text_with_a_nul_as_autocad_does() {
    let bytes = office_ctb().to_bytes();
    let (text, len) = unpacked(&bytes);
    assert_eq!(text.last(), Some(&0));
    assert!(text.ends_with(b"}\n\0"));
    assert_eq!(len, text.len());
    assert!(!text.contains(&b'\r'));
}

/// The checked-in fixtures (written by Qiyas's own writer in AutoCAD's layout) load with their
/// pens.
#[test]
fn the_checked_in_fixtures_load() {
    let ctb = PlotStyle::load(&fixture("office.ctb")).unwrap();
    assert_eq!(ctb, office_ctb());
    let bytes = std::fs::read(fixture("office.ctb")).unwrap();
    let (text, len) = unpacked(&bytes);
    assert_eq!((text.last(), len), (Some(&0), text.len()));
    let pen = ctb.apply([255, 0, 0], None);
    assert_eq!(
        (pen.rgb, pen.lineweight_mm, pen.plot),
        ([0, 0, 0], Some(0.5), true)
    );
    assert!(!ctb.apply([0, 255, 0], None).plot);

    let stb = PlotStyle::load(&fixture("named.stb")).unwrap();
    assert_eq!(stb, named_stb());
    assert_eq!(stb.kind, StyleKind::Stb);
    assert_eq!(stb.apply([255, 0, 0], Some("Heavy")).rgb, [0, 0, 0]);
    assert_eq!(stb.apply([255, 0, 0], None).rgb, [255, 0, 0]);
}
