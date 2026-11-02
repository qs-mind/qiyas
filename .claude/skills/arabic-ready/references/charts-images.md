# Charts and images with Arabic text

Contents: Decide the shaping path · matplotlib · Pillow · SVG · Canvas · Browser chart libraries · OpenCV and others · Layout of RTL charts · Social images and captions

Every technique marked *(verified)* was run and its output inspected. Versions: matplotlib 3.11.2 and 3.10.9, Pillow 12.3.0 with libraqm 0.10.5, Chromium 141. Evidence: `evals/verify/out/`.

## Decide the shaping path first

Run once (Python standard library only):
```bash
python <skill-dir>/scripts/shape_text.py --check
```
It prints, e.g.:
```
matplotlib   -> use raw text (native shaping)
pillow       -> use raw text (native shaping)
reportlab    -> pass Arabic through prepare()
```

| Renderer | Raw Arabic string | Needs `prepare()` (reshape + bidi) |
|---|---|---|
| matplotlib **≥ 3.11** (bundles libraqm) | ✅ *(verified)* | ❌ **breaks it**: reversed again *(verified)* |
| matplotlib ≤ 3.10 | ❌ disconnected letters *(verified)* | ✅ *(verified)* |
| Pillow with raqm (`features.check("raqm")` is True) + `Layout.RAQM` | ✅ *(verified)* | ❌ breaks it *(verified)* |
| Pillow `Layout.BASIC` / no raqm | ❌ *(verified)* | ✅ *(verified)* |
| SVG / canvas / HTML charts in a browser | ✅ (set direction, below) | ❌ never |
| OpenCV `cv2.putText` | ❌ Hershey fonts have no Arabic glyphs at all | Draw text with Pillow instead |

`prepare(text, target)` from `scripts/shape_text.py` applies this table for you. It returns the string unchanged when the target shapes natively:
```python
import sys; sys.path.insert(0, "<skill-dir>/scripts")
from shape_text import prepare
ax.set_title(prepare("المبيعات الشهرية", target="matplotlib"))   # no-op on 3.11+, shaped on 3.10
```
Without the script: `pip install arabic-reshaper python-bidi`, then
`get_display(arabic_reshaper.ArabicReshaper(configuration={"delete_harakat": False}).reshape(s), base_dir="R")`.
The default `arabic_reshaper.reshape()` silently **deletes tashkeel**.

## matplotlib

Always register a real Arabic font. Then pass raw strings on 3.11+:
```python
import matplotlib.pyplot as plt
from matplotlib import font_manager
font_manager.fontManager.addfont("IBMPlexSansArabic-Regular.ttf")   # any Arabic TTF
plt.rcParams["font.family"] = "IBM Plex Sans Arabic"
plt.rcParams["text.language"] = "ar"         # 3.11+: tells libraqm the language

fig, ax = plt.subplots()
ax.bar(["يناير", "فبراير", "مارس"], [120, 180, 150])
ax.set_title("المبيعات الشهرية — فرع الرياض")
ax.set_ylabel("الإيرادات (ألف ريال)")
fig.savefig("chart.png", dpi=150)
```
- ❌ `font.family` left at DejaVu Sans: Arabic renders, but in a poor fallback design.
- ❌ On 3.11+, `get_display(reshape(s))`: reversed, disconnected output *(verified, `matplotlib_fixed.png`)*.
- ✅ ≤ 3.10: wrap **every** string (title, axis labels, tick labels, legend labels, annotations) with `prepare(s, "matplotlib")`.
- Check the version in code: `tuple(map(int, matplotlib.__version__.split(".")[:2])) >= (3, 11)`.
- PDF backend on 3.11 shows a small spacing glitch next to Latin text + `،` *(verified)*. Prefer PNG/SVG for Arabic-heavy charts, or check the PDF.

RTL chart layout *(verified, `matplotlib_rtl_chart.png`)*:
```python
ax.invert_xaxis()                                   # categories start on the right (not for time axes)
ax.yaxis.tick_right(); ax.yaxis.set_label_position("right")
ax.legend(loc="upper left", markerfirst=False)      # marker to the right of its label
AR = str.maketrans("0123456789", "٠١٢٣٤٥٦٧٨٩")       # only if the audience uses Arabic-Indic digits
from matplotlib.ticker import FuncFormatter
ax.yaxis.set_major_formatter(FuncFormatter(lambda v, _: f"{v:,.0f}".translate(AR)))
```
(For Arabic-Indic digits also replace `,` with `٬` and `.` with `٫`, or format with `babel`.)

## Pillow

```python
from PIL import Image, ImageDraw, ImageFont, features

text = "عرض خاص: خصم 25% على جميع المنتجات"
img = Image.new("RGB", (1080, 1080), "white"); d = ImageDraw.Draw(img)

if features.check("raqm"):
    font = ImageFont.truetype("NotoNaskhArabic-Regular.ttf", 56, layout_engine=ImageFont.Layout.RAQM)
    d.text((1040, 80), text, font=font, fill="black", anchor="ra", direction="rtl", language="ar")
else:
    from shape_text import prepare
    font = ImageFont.truetype("NotoNaskhArabic-Regular.ttf", 56, layout_engine=ImageFont.Layout.BASIC)
    d.text((1040, 80), prepare(text, force=True), font=font, fill="black", anchor="ra")
```
- `anchor="ra"` = the *right* edge of the text sits at x (right-aligned, top-anchored). This is how to right-align RTL text in Pillow *(verified)*.
- ❌ RAQM layout + `prepare()`ed text → reversed *(verified, `pillow_raqm_plus_reshape_DOUBLE.png`)*.
- Wrapping long captions: wrap the **logical** text by measured width, then draw line by line *(verified, `pillow_caption_raqm.png`)*:
```python
def wrap_rtl(draw, text, font, max_width):
    lines, cur = [], ""
    for word in text.split():
        trial = f"{cur} {word}".strip()
        if draw.textlength(trial, font=font, direction="rtl", language="ar") <= max_width:
            cur = trial
        else:
            lines.append(cur); cur = word
    return lines + [cur]

y = 80
for line in wrap_rtl(d, text, font, 1000):
    d.text((1040, y), line, font=font, fill="black", anchor="ra", direction="rtl", language="ar")
    y += int(font.size * 1.7)        # Arabic needs generous line spacing
```
  Without raqm: wrap the logical text the same way (measure `prepare(trial, force=True)`), then `prepare()` each line. Never wrap a string that is already shaped, because the lines come out in reverse order.
- `multiline_text(..., align="right", direction="rtl", anchor="ra")` works with raqm for short explicit `\n` text *(verified)*.
- raqm availability: Linux wheels usually have it. Some Windows/macOS installs lack FriBiDi, in which case `features.check("raqm")` is False and you use the BASIC path.

## SVG *(verified in Chromium)*
❌ no direction → `؟` and `Acme` land on the wrong side
```svg
<text x="390" y="30" text-anchor="end">فاتورة رقم 1024 للعميل Acme؟</text>
```
❌ `direction="rtl"` + `text-anchor="end"`: in RTL, "end" is the *left* edge, so the text runs off to the right of x
✅ `direction="rtl"` + `text-anchor="start"` → the right edge sits at x
```svg
<svg xmlns="http://www.w3.org/2000/svg" width="400" height="60" font-family="IBM Plex Sans Arabic, Noto Sans Arabic, sans-serif">
  <text x="390" y="35" direction="rtl" text-anchor="start" unicode-bidi="embed" font-size="20">فاتورة رقم 1024 للعميل Acme؟</text>
</svg>
```
- Center labels: `text-anchor="middle"` works in either direction.
- Standalone SVG files depend on the viewer's fonts. Embed a `<style>@font-face{…}</style>` or convert text to paths for logos.
- SVG → PNG: render in Chromium (Playwright screenshot). Other rasterizers (rsvg, cairosvg, Inkscape) were not tested here.

## Canvas *(verified in Chromium)*
```js
await document.fonts.load('24px "IBM Plex Sans Arabic"');   // otherwise the first draw uses a fallback font
const ctx = canvas.getContext("2d");
ctx.font = '24px "IBM Plex Sans Arabic"';
ctx.direction = "rtl";        // ❌ without it, mixed text reorders like LTR
ctx.textAlign = "right";      // or "start" (= right when direction is rtl)
ctx.fillText("فاتورة رقم 1024 للعميل Acme؟", canvas.width - 10, 40);
```

## Browser chart libraries (Chart.js, ECharts, Recharts, Plotly, Highcharts)
These render through the browser, so shaping is automatic. What you control:
- Page `dir="rtl"` + an Arabic font (`Chart.defaults.font.family`, ECharts `textStyle.fontFamily`, Plotly `layout.font.family`).
- Legend/tooltip direction: Chart.js `plugins.legend.rtl: true`, `plugins.tooltip.rtl: true`, `textDirection: "rtl"`. Highcharts `chart.style.direction` / `useHTML`. *(API names from the libraries' docs, not render-tested here.)*
- Axis position: put the value axis on the right for category charts (`position: "right"` / `yaxis.side: "right"`), and reverse category order if the first category should be on the right (`reverse: true` / `inverse: true` / `autorange: "reversed"`).
- Plotly static export (kaleido) and similar headless exports use Chromium, so the same rules apply.

## OpenCV and others
- `cv2.putText` cannot draw Arabic. Convert the frame to Pillow (`Image.fromarray(cv2.cvtColor(frame, cv2.COLOR_BGR2RGB))`), draw with the Pillow recipe above, then convert back.
- ImageMagick/Wand, reportlab graphics, PyMuPDF `insert_text`: treat as glyph-by-glyph (use `prepare()`) unless you've seen them shape.
- If unsure whether a renderer shapes: draw `"لا إله"` once. Connected letters with the `لا` ligature on the right means native shaping. Separate letters or a reversed order means use `prepare()`.

## Layout of RTL charts

| Flip | Keep |
|---|---|
| Category order in bar charts (first category on the right) when ranking/categories have no natural axis | Time-series x-axis: past on the left, future on the right (universal convention in Arabic finance/news) |
| Value axis to the right side | Numbers themselves and their digit order |
| Legend on the right / top-right, marker to the right of the label (`markerfirst=False`) | Pie/donut segment order (clockwise from 12) |
| Title and annotations right-aligned | Scatter/line chart geometry |
| Horizontal bar charts: labels on the right, bars grow leftwards (`ax.invert_xaxis()`) | Maps, scientific plots |

## Social images and captions
- Best quality: build the image as HTML (web.md rules) and screenshot it with Playwright at the target size (1080×1080, 1200×630). You get full shaping, fonts and bidi.
- Pillow: the raqm path above, with right-aligned text, 1.6–1.8× line spacing, and padding ≥ 6% of the width on both sides.
- Keep Latin URLs/handles on their own line, or isolate them, so they don't reorder the Arabic around them.
- Check the result by opening the PNG and looking at it before delivering.
