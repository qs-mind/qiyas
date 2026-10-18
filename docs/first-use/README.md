# First take-off / أول حصر

A 12-step desktop route, in English and Arabic. Use a copy of `samples/test-plan.dwg`
and keep `samples/test-boq.xlsx` beside it. These are controlled sample files, not a
customer drawing. The drawing is in metres. Coordinates below are drawing coordinates.
The route uses the default ribbon; Settings → Customize can change which tools appear.
At a narrow width, open the group's ⋯ menu to find a hidden command by name.

| Step | English | العربي |
| --- | --- | --- |
| 1 | Start Qiyas. Choose English or Arabic with the language control. | شغّل قياس، واختر العربية أو الإنجليزية من زر اللغة. |
| 2 | Click **Open…** in the top bar and open your copy of `test-plan.dwg`. Check the unit is metres. | اضغط **فتح…** في الشريط العلوي وافتح نسخة `test-plan.dwg`. تأكد أن الوحدة متر. |
| 3 | Keep the **Measure** tab open. Choose **Select**. | افتح تبويب **القياسات** واختر **تحديد**. |
| 4 | Click the partition at (7,4), then press Enter. Its **9.000 m** measurement is added directly to **Take-off**. No separate Add button is needed. | انقر الحائط الفاصل عند (7,4)، ثم Enter. يظهر قياس **9.000 م** في **الحصر** مباشرة؛ لا يوجد زر إضافة منفصل. |
| 5 | Click the living-room outline at (0.15,4), then Enter. Check **58.290 m²** in Take-off. Pick the closed room outline, not the fountain circle. | انقر حدود غرفة المعيشة عند (0.15,4)، ثم Enter. تأكد من **58.290 م²** في الحصر. اختر حدود الغرفة المغلقة، لا دائرة النافورة. |
| 6 | Double-click each new row's description to give it a useful name, such as “Partition 9 m” and “Living room floor”. Enter saves it. | انقر اسم كل صف مرتين لتسميته، مثل «حائط فاصل 9 م» و«أرضية المعيشة». Enter يحفظ الاسم. |
| 7 | To check the complete sample BOQ, add the other measurements listed below. Esc cancels the current tool; Ctrl+Z removes a wrong measurement. | لاستكمال مقارنة جدول العينة كله، أضف القياسات المذكورة أدناه. Esc يلغي الأداة الحالية، وCtrl+Z يتراجع عن قياس خاطئ. |
| 8 | Click **Import BOQ**, choose `test-boq.xlsx`, and check the six codes in the BOQ panel. | اضغط **استيراد جدول الكميات** واختر `test-boq.xlsx`، وتأكد من الأكواد الستة في لوحة جدول الكميات. |
| 9 | In **Take-off**, hover or focus each row, click its chain **Link** button, search for its BOQ code, and choose the matching item. Use the mapping below. | في **الحصر**، مرّر المؤشر على الصف أو حدده بلوحة المفاتيح، واضغط زر السلسلة **ربط**. ابحث بكود البند واختره حسب الجدول أدناه. |
| 10 | Click **Compare**. The conflicts panel must show the two quantity mismatches below. Extra missing items mean a measurement or link is missing. | اضغط **قارن**. تعرض لوحة التعارضات فرقَي الكميات أدناه. ظهور بنود ناقصة إضافية يعني أن قياسًا أو رابطًا ناقص. |
| 11 | Click **Export take-off**, choose a writable location and save an `.xlsx` file. Open it and check the seven rows and totals by measurement kind. **Export conflicts** saves a separate comparison report. | اضغط **تصدير الحصر**، واختر مكانًا قابلًا للكتابة واحفظ ملف `.xlsx`. افتحه وتأكد من الصفوف السبعة والإجماليات حسب نوع القياس. **تصدير التعارضات** يحفظ تقرير المقارنة في ملف منفصل. |
| 12 | Save the drawing with Ctrl+S. Reopen your copy and check the take-off and BOQ links remain. Keep the original sample untouched. | احفظ الرسمة بـCtrl+S، ثم أعد فتح النسخة وتأكد من بقاء الحصر وروابط جدول الكميات. احتفظ بملف العينة الأصلي. |

Step 7 details / تفاصيل الخطوة 7:

- **Distance / مسافة**: click (0,0), (12,0), (12,9), (0,9), (0,0), then Enter: **42.000 m**.
- **Select / تحديد**: click partition / الحائط at (10,5), Enter: **5.000 m**.
- **Select / تحديد**: click bedroom outline / حدود غرفة النوم at (11.85,2.5), Enter: **22.090 m²**.
- **Count / عدد**: click one DOOR block / بلوك باب: **3 pcs / قطعة**.
- **Count / عدد**: click one WINDOW block / بلوك شباك: **5 pcs / قطعة**.

These are 12 workflow stages, including file dialogs, row edits and repeated measurements;
they are not a claim of 12 clicks. No AI account is needed.

| BOQ code / كود البند | Link measurements / القياسات المرتبطة |
| --- | --- |
| A.1 | Outer perimeter / المحيط الخارجي: 42.000 m |
| A.2 | Both partitions / الحائطان الفاصلان: 9.000 + 5.000 = 14.000 m |
| A.3 | Living room / المعيشة: 58.290 m² |
| A.4 | Bedroom / غرفة النوم: 22.090 m² |
| B.1 | Doors / الأبواب: 3 pcs |
| B.2 | Windows / الشبابيك: 5 pcs |

Expected conflicts / التعارضات المتوقعة:

- A.4: BOQ 30.000 m²; measured / المقاس 22.090 m²; −26.4%.
- B.2: BOQ 4 pcs; measured / المقاس 5 pcs; +25.0%.

## Verification state / حالة التحقق

Implementation: headline ribbon buttons now show translated names beside their icons;
import and take-off export use distinct table-and-arrow icons. The empty Take-off panel
explains Select → click → Enter in both languages.

Pending desktop evidence: capture start, open drawing, wall measurement, room measurement,
BOQ links, comparison, exported workbook, and the Arabic ribbon. Record OS, app revision,
installer source and observed values. Attach screenshots to the issue. A component test or a
browser rendering does not prove native dialogs, DWG loading or Excel export.

Windows: add a Windows screenshot only after a CI installer is confirmed and actually run
on Windows. No Windows installation or screenshot has been verified in this change.
