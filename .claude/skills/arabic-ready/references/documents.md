# Documents: Word, PowerPoint, PDF

Contents: Word with python-docx · Word with docx-js · PowerPoint with python-pptx · PowerPoint with pptxgenjs · PDF: which libraries shape Arabic · PDF recipes · Checking the result

Every snippet here was run and the output rendered and inspected (LibreOffice 24.2 for Office files, PyMuPDF to rasterize PDFs). Versions and before/after images: `evals/verify/README.md`. **Microsoft Word / PowerPoint themselves were not available for testing.** The XML used is the standard OOXML that Word writes for Arabic documents.

## The three things Office needs (both docx and pptx)
1. **Paragraph direction**: docx `<w:bidi/>` in `w:pPr`; pptx `rtl="1"` on `a:pPr`.
2. **Run direction + complex-script properties**: docx `<w:rtl/>`, `w:rFonts/@w:cs`, `w:szCs`, `w:bCs`; pptx `a:cs` font + `lang="ar-SA"`.
3. **Alignment from direction**: in an RTL paragraph, leave docx alignment unset (start = right). In pptx set `algn="r"`.

The trap *(verified)*: Word applies `w:sz` and `w:b` to Latin text only. Arabic text uses **`w:szCs` and `w:bCs`**. Without them, Arabic stays at the default size and is not bold.
❌ `run.font.size = Pt(28); run.bold = True` → only the Latin letters change
✅ also add `<w:szCs w:val="56"/>` and `<w:bCs/>`

## Word with python-docx (tested 1.2.0)

Set RTL **once** in the document defaults. Headings, lists, table cells and headers then inherit it *(verified, `docx_defaults.png`)*:

```python
from docx import Document
from docx.oxml import OxmlElement
from docx.oxml.ns import qn

THEME_ATTRS = ("w:asciiTheme", "w:hAnsiTheme", "w:cstheme", "w:eastAsiaTheme")

def make_rtl_document(font="Arial", size_pt=12, lang="ar-SA"):
    """New Document whose every paragraph is RTL with an Arabic complex-script font."""
    doc = Document()
    defaults = doc.styles.element.find(qn("w:docDefaults"))
    rpr = defaults.find(qn("w:rPrDefault")).find(qn("w:rPr"))
    fonts = rpr.find(qn("w:rFonts"))
    for a in THEME_ATTRS:
        fonts.attrib.pop(qn(a), None)
    for a in ("w:ascii", "w:hAnsi", "w:cs"):
        fonts.set(qn(a), font)
    for tag in ("w:sz", "w:szCs"):
        el = rpr.find(qn(tag))
        if el is None:
            el = OxmlElement(tag); rpr.append(el)
        el.set(qn("w:val"), str(size_pt * 2))
    lang_el = rpr.find(qn("w:lang"))
    if lang_el is None:
        lang_el = OxmlElement("w:lang"); rpr.append(lang_el)
    lang_el.set(qn("w:bidi"), lang)
    rpr.append(OxmlElement("w:rtl"))
    ppr_default = defaults.find(qn("w:pPrDefault"))
    if ppr_default is None:
        ppr_default = OxmlElement("w:pPrDefault"); defaults.append(ppr_default)
    ppr = ppr_default.find(qn("w:pPr"))
    if ppr is None:
        ppr = OxmlElement("w:pPr"); ppr_default.append(ppr)
    ppr.insert(0, OxmlElement("w:bidi"))
    # Heading/Title styles carry theme fonts: point them at the Arabic font too
    for st in doc.styles:
        st_rpr = st.element.find(qn("w:rPr"))
        if st_rpr is not None and st_rpr.find(qn("w:rFonts")) is not None:
            f = st_rpr.find(qn("w:rFonts"))
            for a in THEME_ATTRS:
                f.attrib.pop(qn(a), None)
            for a in ("w:ascii", "w:hAnsi", "w:cs"):
                f.set(qn(a), font)
    return doc

def bold(run):                       # bold that also applies to Arabic
    run.bold = True
    run._r.get_or_add_rPr().append(OxmlElement("w:bCs"))

def size(run, pt):                   # size that also applies to Arabic
    rpr = run._r.get_or_add_rPr()
    for tag in ("w:sz", "w:szCs"):
        el = OxmlElement(tag); el.set(qn("w:val"), str(int(pt * 2))); rpr.append(el)

def rtl_table(table):                # first column on the right
    table._tbl.tblPr.append(OxmlElement("w:bidiVisual"))

doc = make_rtl_document(font="Arial")          # Arial/Tahoma exist on every Word install
doc.add_heading("تقرير المبيعات الربعي", level=1)
p = doc.add_paragraph("ارتفعت المبيعات بنسبة 25% مقارنة بالربع السابق.")
bold(p.add_run(" نتيجة ممتازة."))
doc.add_paragraph("افتتاح فرعين جديدين", style="List Bullet")   # bullet on the right
t = doc.add_table(rows=2, cols=3); t.style = "Table Grid"; rtl_table(t)
doc.save("report.docx")
```
Notes:
- ❌ `paragraph.alignment = WD_ALIGN_PARAGRAPH.RIGHT` on RTL paragraphs: unnecessary, and alignment semantics in bidi paragraphs differ between Word versions. Leave it unset (start = right). `CENTER` is safe.
- Signed numbers in cells (`+12%`, `-5`) display as `12%+` in an RTL paragraph *(verified)*. Prefix LRM: `"‎+12%"`.
- Phone numbers/emails inside Arabic paragraphs: wrap in LRI/PDI (`"⁦+966 50 123 4567⁩"`), see bidi.md.
- Per-paragraph alternative (editing an existing LTR document): add `<w:bidi/>` as the first child of `w:pPr`, and to each run add `<w:rtl/>`, `w:rFonts/@w:cs`, `w:szCs`, `w:bCs` as needed *(verified, `docx_fixed.png`)*.
- Justified text: `w:jc w:val="both"`. LibreOffice adds kashida automatically for Arabic.
- Never set character spacing (`w:spacing` in `w:rPr`) on Arabic runs, because it breaks joining *(verified)*.

## Word with docx-js (`docx` npm, tested 9.12.0)
```js
import { Document, Packer, Paragraph, TextRun, Table, TableRow, TableCell, WidthType } from "docx";

const FONT = { ascii: "Arial", hAnsi: "Arial", cs: "Arial" };
const run = (text, o = {}) => new TextRun({ text, rightToLeft: true, font: FONT, size: 24, sizeComplexScript: 24, ...o });
const P = (text, o = {}) => new Paragraph({ bidirectional: true, children: [run(text, o)] });

const doc = new Document({ sections: [{ children: [
  P("تقرير المبيعات", { bold: true, boldComplexScript: true, size: 36, sizeComplexScript: 36 }),
  P("فاتورة رقم 1024 للعميل Acme، الإجمالي 49.99 دولار؟"),
  new Table({
    visuallyRightToLeft: true,                       // first column on the right
    rows: [new TableRow({ children: ["المنتج", "الكمية"].map(t =>
      new TableCell({ width: { size: 3000, type: WidthType.DXA }, children: [P(t)] })) })],
  }),
]}]});
```
❌ `bold: true` alone, `size` alone → Arabic not bold / default size
✅ `boldComplexScript: true`, `sizeComplexScript: …`

## PowerPoint with python-pptx (tested 1.0.2)
```python
from pptx import Presentation
from pptx.util import Pt
from pptx.oxml.ns import qn
from lxml import etree

def rtl_paragraph(paragraph, font="Arial", lang="ar-SA"):
    pPr = paragraph._p.get_or_add_pPr()
    pPr.set("rtl", "1")
    pPr.set("algn", "r")                       # RTL alone doesn't right-align the paragraph
    for r in paragraph.runs:
        rPr = r._r.get_or_add_rPr()
        rPr.set("lang", lang)
        for tag in ("a:latin", "a:cs"):        # schema order: latin, ea, cs
            el = rPr.find(qn(tag))
            if el is None:
                el = etree.SubElement(rPr, qn(tag))
            el.set("typeface", font)

prs = Presentation()
slide = prs.slides.add_slide(prs.slide_layouts[6])
tf = slide.shapes.add_textbox(Pt(36), Pt(36), Pt(640), Pt(200)).text_frame
tf.word_wrap = True
for i, line in enumerate(["نتائج الربع الثالث", "نمو 25% مقارنة بالعام الماضي"]):
    p = tf.paragraphs[0] if i == 0 else tf.add_paragraph()
    p.add_run().text = line
    p.runs[0].font.size = Pt(32 if i == 0 else 20)
    rtl_paragraph(p)
prs.save("deck.pptx")
```
Placeholders from layouts (title/body): call `rtl_paragraph` on every paragraph after setting text. Bullets from the layout then appear on the right.
Without `rtl="1"`, mixed lines reorder (`؟` and `Acme` jump) and bullets stay on the left *(verified, `pptx_naive.png`)*.
Mirror the *layout* too: put the title block, logos and image/text columns in the mirrored position (image on the left, text on the right).

## PowerPoint with pptxgenjs (tested 4.0.1)
❌ *(verified)* `rtlMode` only on the `addText` call when text is an array of runs. `rtl="1"` is not emitted.
```js
slide.addText([{ text: "نتائج", options: { breakLine: true } }, { text: "نمو 25%" }], { rtlMode: true, align: "right" });
```
✅ put it in every run's options (and the outer options):
```js
const ar = { rtlMode: true, align: "right", lang: "ar-SA", fontFace: "Arial" };
slide.addText([
  { text: "نتائج الربع الثالث", options: { ...ar, fontSize: 28, breakLine: true } },
  { text: "نمو 25% مقارنة بالعام الماضي", options: { ...ar, fontSize: 20, bullet: true } },
], { x: 0.5, y: 0.5, w: 9, h: 2, valign: "top", ...ar });
slide.addText("نص من سطر واحد", { x: 0.5, y: 3, w: 9, h: 0.6, ...ar });   // plain string: outer options suffice
```

## PDF: which libraries shape Arabic

| Library | Out of the box | Working approach | Verified |
|---|---|---|---|
| **Chromium** (Playwright/Puppeteer `page.pdf`) | ✅ | HTML with `dir="rtl"` + Arabic web font. Pass `preferCSSPageSize: true` to honor `@page { size }` | ✅ |
| **WeasyPrint** (70.0) | ✅ (Pango/HarfBuzz) | HTML/CSS with `dir="rtl"`, `@font-face` | ✅ |
| **LibreOffice** (`soffice --headless --convert-to pdf`) | ✅ | Build a correct RTL docx/pptx first | ✅ |
| **PyMuPDF** `insert_htmlbox` (1.28) | ✅ | `<p dir="rtl">` + font via `fitz.Archive` + CSS | ✅ |
| **PyMuPDF** `insert_text` / `insert_textbox` | ❌ disconnected, reversed | Use `insert_htmlbox` | ✅ |
| **fpdf2** (2.8.9) | ❌ | `pdf.set_text_shaping(use_shaping_engine=True, direction="rtl", script="arab", language="ar")`, which needs `pip install uharfbuzz`. `cell` and `multi_cell` wrapping both work | ✅ |
| **reportlab** (5.0.1) | ❌ | `prepare()` from `scripts/shape_text.py` + `drawRightString`. Multi-line: wrap logical text yourself, one Paragraph per line (below) | ✅ |
| reportlab native RTL (`rlbidi`) | not tested | `rlbidi` is not on PyPI | ⚪ |
| matplotlib `PdfPages` (3.11) | ⚠️ shaped, small spacing glitch near Latin + `،` | Fine for charts. Use PNG/SVG if it shows | ✅ |
| jsPDF, pdf-lib, PDFKit (JS), wkhtmltopdf | not tested | Prefer Chromium `page.pdf()` in JS stacks | ⚪ |

**Default choice:** generate HTML (styled with web.md rules) and print it with Chromium or WeasyPrint. You get correct shaping, bidi, line wrapping, fonts and tables for free.

## PDF recipes

### HTML → PDF with Playwright (Python)
```python
from playwright.sync_api import sync_playwright
html = """<!doctype html><html lang="ar" dir="rtl"><meta charset="utf-8">
<style>@page { size: A4; margin: 18mm } body { font-family: "Noto Naskh Arabic", "Amiri", serif; line-height: 1.8 }</style>
<h1>فاتورة رقم 1024</h1><p>الهاتف: <bdi dir="ltr">+966 50 123 4567</bdi></p></html>"""
with sync_playwright() as p:
    page = p.chromium.launch().new_page()
    page.set_content(html, wait_until="networkidle")      # waits for Google Fonts
    page.pdf(path="invoice.pdf", prefer_css_page_size=True, print_background=True)
```

### WeasyPrint
```python
import weasyprint
weasyprint.HTML(string=html, base_url=".").write_pdf("invoice.pdf")   # same HTML as above, local @font-face works
```

### fpdf2
```python
from fpdf import FPDF                     # pip install fpdf2 uharfbuzz
pdf = FPDF(); pdf.add_page()
pdf.add_font("Naskh", "", "NotoNaskhArabic-Regular.ttf")
pdf.set_font("Naskh", size=14)
pdf.set_text_shaping(use_shaping_engine=True, direction="rtl", script="arab", language="ar")
pdf.cell(w=0, h=10, text="فاتورة رقم 1024 للعميل Acme", align="R", new_x="LMARGIN", new_y="NEXT")
pdf.multi_cell(w=0, h=8, text=long_arabic_text, align="R")
pdf.output("invoice.pdf")
```
❌ Running `prepare()` / arabic-reshaper on text *and* enabling `set_text_shaping`: that shapes twice.

### reportlab
```python
from reportlab.pdfgen import canvas
from reportlab.pdfbase import pdfmetrics
from reportlab.pdfbase.ttfonts import TTFont
from shape_text import prepare            # skill script; or arabic_reshaper + bidi.get_display
pdfmetrics.registerFont(TTFont("Naskh", "NotoNaskhArabic-Regular.ttf"))
c = canvas.Canvas("out.pdf"); c.setFont("Naskh", 14)
c.drawRightString(560, 800, prepare("فاتورة رقم 1024 للعميل Acme", target="reportlab"))  # right-anchored
c.save()
```
Multi-line paragraphs *(verified)*:
❌ `Paragraph(prepare(long_text), style)`: reportlab wraps the *visual* string, so lines come out bottom-to-top.
❌ wrapping to the full `doc.width`: a line that is slightly too wide gets re-wrapped by reportlab, and its first word drops to the next line *(verified)*.
✅ wrap the logical text first, then shape each line:
```python
from reportlab.pdfbase.pdfmetrics import stringWidth
from reportlab.platypus import Paragraph
from reportlab.lib.styles import ParagraphStyle
from reportlab.lib.enums import TA_RIGHT

def wrap_rtl(text, font, size, width):
    lines, cur = [], ""
    for word in text.split():
        trial = f"{cur} {word}".strip()
        if stringWidth(prepare(trial, "reportlab"), font, size) <= width:
            cur = trial
        else:
            lines.append(cur); cur = word
    return lines + [cur]

style = ParagraphStyle("ar", fontName="Naskh", fontSize=12, leading=21, alignment=TA_RIGHT)
frame_width = doc.width - 12         # SimpleDocTemplate frames have 6pt padding per side
story = [Paragraph(prepare(line, "reportlab"), style) for line in wrap_rtl(long_text, "Naskh", 12, frame_width)]
```
reportlab Tables: put `prepare()`d strings in cells, reverse the column order yourself (first column on the right), and use `('ALIGN', (0,0), (-1,-1), 'RIGHT')`.

### PyMuPDF
```python
import pymupdf
doc = pymupdf.open(); page = doc.new_page()
page.insert_htmlbox(pymupdf.Rect(40, 40, 555, 200),
    '<p dir="rtl" style="font-size:14px">فاتورة رقم 1024 للعميل Acme</p>',
    css="@font-face {font-family: naskh; src: url(NotoNaskhArabic-Regular.ttf);} p {font-family: naskh;}",
    archive=pymupdf.Archive("/path/to/font/folder"))
doc.save("out.pdf")
```

## Checking the result
- docx/pptx → PDF → PNG: `soffice --headless --convert-to pdf file.docx`, then `pymupdf` `page.get_pixmap(matrix=pymupdf.Matrix(2, 2)).save("p.png")`, and look at it.
- Text extraction (`page.get_text()`) of Arabic PDFs is often scrambled even when the rendering is perfect. **Judge by the rendered image, not by extracted text.**
- Look for: connected letters, first word on the right, `؟` at the left end, numbers in the right order, table first column on the right, Arabic bold/size applied.
