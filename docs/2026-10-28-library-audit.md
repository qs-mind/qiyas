# مراجعة: كود مكتوب يدويًا وله مكتبة جاهزة (28 أكتوبر 2026)

كتبها كودكس بقراءة الكود كله (من غير تشغيل). الرخص اتأكد منها على crates.io و npm يوم 28 أكتوبر: كل المكتبات المقترحة MIT أو Apache-2.0 أو BSD أو BSL-1.0، يعني مسموحة. clipper2-rust (BSL-1.0) و tiny-skia (BSD-3) لازم نص الرخصة يتحط في LICENSES/.

لقيت **32 موضع يستحق الاستبدال أو دراسة الاستبدال: 23 «replace now»، و9 «replace later»**. أهمهم متعلق بصحة المساحات، التقاطعات، والثقوب، وبعدهم التعامل مع النص.

عملت مسحًا لكل الـ crates و`src/`، وفتحت التنفيذات المذكورة تحت. المراجعة ساكنة فقط: **مفيش تعديل ملفات، build، tests، أو اتصال بالشبكة**. أرقام السطور تقريبية، وحجم الكود هو الجزء المعني، مش وعد بعدد السطور اللي هتتحذف.

«المصدر موجود» يعني لقيته في `~/.cargo/registry/src` أو `node_modules`، مش بالضرورة dependency مباشرة. تراخيص النسخ المحلية راجعتها من manifests؛ المكتبات الغائبة وتوافق بعض واجهاتها معلّمة **verify**. حالة الصيانة الحالية على الإنترنت ما اتراجعتش. ظهور مكتبة مستخدمة بالفعل هنا معناه وجود تنفيذ يدوي متبقّي بجانبها، مش إعادة اقتراح الاستبدالات المحسومة.

1. **اختبارات التقاطع والاحتواء والهندسة الأساسية المتكررة: replace now؛ medium.**

   **المكان:** [measurement_scope.rs:248](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/src/measurement_scope.rs:248): `inside`, `segment_inside`؛ [session.rs:2672](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/src/session.rs:2672): `in_ring`, `on_edge`, `turn`, `cross_properly`, `touches_ring`؛ [ink.rs:374](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/src/ink.rs:374): `inside`, `crosses`. فيه نسخ إضافية في `check.rs:1482–1531`، `draw.rs:1312`، `room_objects.rs:672`، `element_metadata.rs:168–238`، و`qiyas-geom/src/walls.rs:1472–1544`، بما فيها `convex_hull`. إجمالًا نحو **350–450 سطر**.

   **الوظيفة:** تحديد تقاطع القطع، وقوع النقاط داخل حدود، والمراكز والغلاف المحدب باستخدام determinants وray casting مكتوبة يدويًا.

   **البديل:** `geo`: MIT/Apache-2.0، **غير موجود؛ verify للإصدار المختار**؛ و`robust` 1.2: MIT/Apache-2.0، **موجود** للـ orientation predicates.

   **المكسب والمخاطرة:** تحسين التعامل مع الإحداثيات الكبيرة والحالات شبه المتوازية، وإزالة اختلافات التنفيذ بين المسارات. لازم نحافظ على معنى «على الحد»، والـ drawing tolerance؛ `robust` يحسّن إشارة المحدد، لكنه مش بديل لسياسة السماحية أو حساب نقطة التقاطع كاملة.

2. **تفكيك المسارات ذات التقاطع الذاتي قبل حساب المساحة: replace now؛ medium.**

   **المكان:** [path.rs:110](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-geom/src/path.rs:110): `shoelace`, `loops_area`, `split_loops`, `first_crossing`, `segment_hit`، نحو **65 سطر خوارزميات**، بجانب استدعاءاتها في `Path::area/loops`.

   **الوظيفة:** البحث عن أول تقاطع وتقسيم الحلقة recursively ثم جمع مساحات الحلقات.

   **الدليل:** البحث زوجي O(n²)، و`segment_hit` يستبعد التوازي تمامًا، وبالتالي التداخلات على نفس الخط؛ وshoelace يستخدم الإحداثيات المطلقة مباشرة.

   **البديل:** Clipper2 عبر `clipper2-rust` 1.2، **BSL-1.0، موجود**؛ يدعم المسارات المعقدة وfill rules. **verify لنضج وصيانة الـ Rust port**؛ نضج Clipper2 الأصلي مش ضمان تلقائي للـ port.

   **المكسب والمخاطرة:** تغطية أفضل للتقاطعات والتداخلات، لكن اختيار `EvenOdd` أو `NonZero` وطريقة تحويل الإحداثيات للدقة الداخلية ممكن يغيّر المساحة. لازم تعريف السلوك الحالي للـ bow-ties والحلقات المتكررة قبل النقل.

3. **إدارة الثقوب والأجزاء الناتجة فوق مكتبة الـ booleans: replace now؛ medium.**

   **المكان:** [walls.rs:1294](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-geom/src/walls.rs:1294): `union_into`, `cut_from`، نحو **115 سطر**.

   **الوظيفة:** تركيب نتائج booleans المنفّذة بالفعل بواسطة Cavalier في مناطق لها outlines وثقوب.

   **الدليل:** `cut_from` يبدأ من `result.pos_plines` بثقوب فارغة، من غير نقل `result.neg_plines` الناتجة من القطع الأول؛ وفي مراحل لاحقة يوزّع الثقوب حسب أول vertex. ده يستحق أولوية عالية خصوصًا للقطع الداخلي بالكامل؛ لم أشغّل حالة لإثبات أثره على ناتج الحصر.

   **البديل:** `clipper2-rust`: **BSL-1.0، موجود**، باستخدام `PolyTree`/face ownership؛ نفس **verify** الخاص بالـ port.

   **المكسب والمخاطرة:** نقل ملكية الثقوب والجزر للمكتبة بدل bookkeeping يدوي. المخاطر: ترتيب النتائج، winding، precision، وتغيّر حدود المناطق. المطلوب استبدال الطبقة دي، مش قواعد تكوين الحوائط.

4. **تقريب منحنيات Bézier المستخرجة من PDF: replace now؛ small.**

   **المكان:** [vectors.rs:242](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-pdf/src/vectors.rs:242): `bezier`، نحو **21 سطر**.

   **الوظيفة:** تحويل cubic Bézier إلى قطع مستقيمة بتقسيم منتظم.

   **الدليل:** عدد القطع محسوب من حد للانحراف، ثم مقيد بـ1000؛ عند بلوغ السقف، ضمان `FLATNESS_PT` المكتوب في التعليق مش بالضرورة يفضل صحيحًا.

   **البديل:** `kurbo` 0.13.1، **MIT/Apache-2.0، موجود**، باستخدام flattening قائم على tolerance.

   **المكسب والمخاطرة:** خوارزمية عامة مدروسة، وتقسيم أنسب لشكل المنحنى. لازم الحفاظ على استبعاد نقطة البداية، وإدراج النهاية، وتطبيق السماحية في فضاء PDF الصحيح بعد التحويلات؛ مع حد موارد واضح للحالات الشديدة.

5. **لفّ النص وتحديد أماكن كسر السطر: replace now؛ medium.**

   **المكان:** [text.rs:508](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-render/src/text.rs:508): `TextEngine::wrap`؛ [report.rs:51](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-plot/src/report.rs:51): `wrap`، نحو **70 سطر**.

   **الوظيفة:** تقسيم النص إلى كلمات وقياسها لبناء سطور تناسب عرضًا محددًا.

   **الدليل:** الأول يعتمد على `split(' ')`؛ الثاني على `split_whitespace`، ويكسر الكلمات الطويلة عند `char`، مش grapheme.

   **البديل:** `unicode-linebreak` 0.1.5: **Apache-2.0**، و`unicode-segmentation` 1.13.3: **MIT/Apache-2.0**؛ **الاتنين موجودين**.

   **المكسب والمخاطرة:** NBSP، علامات التشكيل، emoji، وحدود السطر بلغات مختلفة. المكتبتان تحددان فرص الكسر؛ قياس النص بـrustybuzz يفضل موجودًا. تغيّر اللف يؤثر على text bounds، ارتفاع الصفوف، وترقيم صفحات التقارير.

6. **خصائص Unicode وتجميع الحروف مع العلامات: replace now؛ medium.**

   **المكان:** [text.rs:718](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-render/src/text.rs:718): `is_arabic`, `is_rtl_char`, `is_mark`, `is_letter`, `is_digit`؛ `visual_pieces` عند 876؛ و`itemize` عند 251. نحو **70–100 سطر** معني.

   **الوظيفة:** تصنيف الحروف وتجميعها وتوزيعها على الخطوط باستخدام ranges وتجميع يدوي.

   **البديل:** `unicode-general-category`: **Apache-2.0**؛ `unicode-script` و`unicode-segmentation`: **MIT/Apache-2.0**؛ **كلهم موجودين**، مع استخدام خصائص `unicode-bidi` الموجود بالفعل.

   **المكسب والمخاطرة:** تغطية أحدث وأوسع للتشكيل والـ combining sequences. لازم الحفاظ على byte offsets والـ shaping clusters. إصلاح النص العربي القديم المخزّن بترتيب بصري يفضل سياسة Qiyas؛ مش مطلوب حذفه أو استبداله بتطبيع Unicode عشوائي.

7. **البحث النصي العام وحساب حدود الكلمات: replace now؛ medium.**

   **المكان:** [text.rs:69](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/src/text.rs:69): `Finder::new`, `same`, `matches`، نحو **40 سطر**.

   **الوظيفة:** بحث literal غير متداخل، مع تجاهل حالة الحروف وخيار whole-word.

   **الدليل:** تجاهل الحالة مبني على lowercase لكل حرف؛ وحد الكلمة هو alphanumeric أو `_`، من غير combining marks.

   **البديل:** `regex` 1.13.1، **MIT/Apache-2.0، موجود ومستخدم بالفعل**؛ pattern escaped مع case-insensitive matching وحدود كلمات محددة بوضوح.

   **المكسب والمخاطرة:** Unicode case folding أفضل وخوارزمية بحث جاهزة. تغيير تعريف الكلمة ممكن يغيّر النتائج؛ ويلزم تحويل byte ranges عند الحاجة. فصل أكواد MText وحمايتها من الاستبدال يفضل كما هو.

8. **اكتشاف ملفات الخطوط واختيار الوجه داخل font collections: replace now؛ medium.**

   **المكان:** [fonts.rs:88](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-render/src/fonts.rs:88): `load`, `file_name`, `system_font`, `font_index`, `font_dirs`, `walk`؛ و`Font::parse` عند 30. نحو **100 سطر**.

   **الوظيفة:** مسح directories للخطوط، مطابقة الأسماء، وتحميل البيانات وتثبيت عمرها.

   **الدليل:** المسح محدود بعمق، والـ parsing يبدأ بالـ face رقم صفر حتى مع ملفات collections.

   **البديل:** `fontdb` 0.23، **MIT، موجود**.

   **المكسب والمخاطرة:** قاعدة بيانات للوجوه والـ collections ومطابقة family/style، وتقليل filesystem plumbing. يلزم تكييف lifetimes والحفاظ على أولوية filename الخاصة بـCAD. `fontdb` لا يوفّر وحده سياسة fallback ولا دعم SHX.

9. **بناء موارد خطوط PDF وخرائط النص القابل للاستخراج: replace later؛ large.**

   **المكان:** [font.rs:233](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-plot/src/font.rs:233): `FontSet::write`، و`to_unicode` عند 350؛ [content.rs:1](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-plot/src/content.rs:1): `Content`؛ نحو **200–300 سطر** من تركيب الموارد وكتابة operators، بخلاف سياسة اختيار النص.

   **الوظيفة:** بناء CID fonts، descriptors، widths، ToUnicode CMaps، وcontent streams يدويًا فوق lopdf/subsetter.

   **البديل:** `krilla` 0.8.2، **MIT/Apache-2.0، موجود**؛ عنده `draw_glyphs` للنص المشكّل مسبقًا وActualText. البديل الأضيق هو `pdf-writer` 0.15، **MIT/Apache-2.0، موجود** لكتابة operators وCMaps.

   **المكسب والمخاطرة:** تقليل مسؤولية تفاصيل PDF وتحسين فرص استخراج النص بشكل صحيح. **verify** لتوافق clusters، النص العربي، transforms، وسياسة embedding. تغيير backend كامل أكبر بكتير من استبدال string formatting، وقد يزيد dependencies.

10. **إعادة أخذ عينات صور الحروف وتركيبها يدويًا: replace now؛ medium.**

    **المكان:** [image.rs:611](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-ai/src/image.rs:611): `glyphs`، نحو **80 سطر**.

    **الوظيفة:** تحويل glyph bitmaps، اختيار texels، وتنفيذ alpha blending لكل pixel في صورة الرسم.

    **البديل:** `tiny-skia` 0.12، **BSD-3-Clause، موجود ومستخدم بالفعل**، عبر transformed pixmap/pattern rendering.

    **المكسب والمخاطرة:** resampling وcompositing جاهزان، وفرصة لتحسين وضوح الحروف الصغيرة والمائلة في الصور المقروءة بواسطة النموذج. يلزم ضبط premultiplied alpha، اتجاه Y، والفلتر؛ وتحويل صفحات R8 إلى صيغة مناسبة ممكن يضيف ذاكرة. ده لا يغيّر shaping ولا يدخل في مهمة OCR.

11. **هندسة offsets والـ mitre joins للحوائط: replace later؛ large.**

    **المكان:** [walls.rs:1142](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-geom/src/walls.rs:1142): `strip_corners`, `strip`, `offset_corner`، نحو **125 سطر معني**.

    **الوظيفة:** إنشاء جانبي مسار الحائط، تقاطع خطوط الإزاحة، وقصّ الزوايا الطويلة.

    **البديل:** offsetting في `clipper2-rust`، **BSL-1.0، موجود**؛ **verify** للـ port ولتطابق joins.

    **المكسب والمخاطرة:** تفويض الحالات العامة ذات العرض الثابت للمكتبة، خصوصًا الانعطافات الحادة والمسارات المغلقة. لكن الكود الحالي يدعم offsets مختلفة لكل segment وخطوات عند تغير السمك، وبيحدّ الـ mitre بقص شعاعي؛ دي مش مطابقة مباشرة لـClipper. يحتفظ Qiyas بقواعد السمك والتقاطعات، والاستبدال يقتصر على الجزء الهندسي المتوافق.

12. **حلقة تحسين Levenberg–Marquardt اليدوية: replace later؛ large.**

    **المكان:** [solver.rs:656](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-param/src/solver.rs:656): `solve_group`، نحو **120 سطر**.

    **الوظيفة:** حساب Jacobian بالفروق المحدودة، اختيار damping، وحل الخطوة وتحديد التقارب.

    **البديل:** `levenberg-marquardt`: **MIT حسب المعروف؛ غير موجود، verify**؛ أو solver مناسب من `argmin`: **MIT/Apache-2.0 حسب المعروف؛ غير موجود، verify**.

    **المكسب والمخاطرة:** تقارب وتشخيص فشل وسياسات iteration مدروسة. العائق الأساسي: Qiyas يستخدم خطوة minimum-change عبر `J Jᵀ` للرسومات under-constrained؛ بعض LM APIs تفترض عدد residuals لا يقل عن المتغيرات. لازم إثبات التوافق قبل الاستبدال. تعريف constraints، تقسيم المجموعات، وتطبيع الوحدات تفضل عندنا.

13. **تعريف عقود Rust/TypeScript مرتين: replace now؛ medium.**

    **المكان:** [api.ts:1](/home/kareem/Work/qs-mind/qiyas/src/api.ts:1): **149 تعريف type/interface** موزعة داخل ملف 2088 سطر؛ مساحة التعريفات اليدوية من رتبة **ألف سطر أو أكتر**.

    **الوظيفة:** إعادة كتابة أشكال البيانات الراجعة من Rust والمتبادلة عبر IPC في TypeScript.

    **البديل:** `ts-rs` 12.0.1، **MIT، موجود**، لتوليد التعريفات من Rust.

    **المكسب والمخاطرة:** أكبر فرصة واضحة لإزالة تكرار ومنع contract drift. لازم ضبط serde renames، `Option` مقابل optional، unions، و`u64` مقابل JavaScript numbers. الـ invoke wrappers، النقل الثنائي، والبيانات الديناميكية تحتاج adapters؛ التوليد مش runtime validation.

14. **تنفيذ أجزاء OAuth/OIDC والتحقق من ID tokens: replace later؛ large.**

    **المكان:** [oauth.rs:90](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-ai/src/chatgpt_plan/oauth.rs:90): `Pending`، و`Protocol` عند 273، و`tokens_from_response` عند 443، و`validate_id_token` عند 519؛ نحو **300–400 سطر بروتوكول عام**.

    **الوظيفة:** بناء تدفق authorization/PKCE، تبادل وتجديد tokens، قراءة discovery/JWKS، والتحقق من JWT claims.

    **البديل:** `openidconnect` و`oauth2`، **MIT/Apache-2.0 حسب المعروف؛ الاتنين غير موجودين، verify** للإصدارات والتوافق.

    **المكسب والمخاطرة:** نقل تفاصيل البروتوكول والتدقيق في claims إلى مكتبات متخصصة. التشفير نفسه بالفعل عبر `ring`؛ المشكلة مش DIY RSA. يلزم توافق إصدار `client_id` أثناء التدفق، resource parameters، refresh rotation، وقيود endpoints. سياسات الاشتراك والصلاحيات الخاصة بالمنتج تفضل عندنا.

15. **تنفيذ JSON Schema validator جزئي: replace now؛ medium.**

    **المكان:** [validation.rs:574](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/src/validation.rs:574): `matches_type`, `validate_schema`, `scalar_choices`, `walk`؛ نحو **330 سطر**.

    **الوظيفة:** تفسير schema وفحص الأنواع والفروع والحقول والحدود وإنتاج الأخطاء.

    **الدليل:** `oneOf` و`anyOf` يدخلوا نفس المسار، ويُختار أفضل فرع؛ ده لا يحقق شرط نجاح فرع واحد بالضبط لـ`oneOf`. وفيه دمج يدوي خاص لـ`allOf`.

    **البديل:** `jsonschema`، **MIT حسب المعروف؛ غير موجود، verify** للإصدار.

    **المكسب والمخاطرة:** صحة semantics وتغطية keywords بدل توسيع validator داخلي باستمرار. يلزم الإبقاء على `$step` references، الرسائل المناسبة للمستخدم، وسياسة warnings، وربطها بالـ validator. لا يُفعّل تحميل remote references تلقائيًا. `schemars` يفضل مسؤولًا عن التوليد.

16. **محرك parsing للتعبيرات: replace later؛ medium.**

    **المكان:** [expr.rs:229](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-param/src/expr.rs:229): `Parser` ومسار `Expr::parse` حتى نحو 570؛ **حوالي 340 سطر**.

    **الوظيفة:** tokenization وprecedence وقراءة الأرقام والنصوص والأقواس واستدعاءات الدوال.

    **البديل:** `winnow` 1.0.4، **MIT، موجود**.

    **المكسب والمخاطرة:** parsing primitives ورسائل أخطاء وإدارة input أكثر اتساقًا. grammar نفسها هتفضل مكتوبة، فمش كل السطور هتختفي. لازم الحفاظ على associativity، unary minus، quoted strings، حدود العمق والعقد، وlazy evaluation. مش باقترح استبدال لغة المنتج بـcalculator لا يدعمها.

17. **تكرار tessellation وقياس المنحنيات بجانب الـ kernel: replace later؛ medium.**

    **المكان:** [flatten.rs:1072](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-geom/src/flatten.rs:1072): `arc_points`, `nurbs_points`, `segment_count`, `ellipse_points`, `bulge_points/interior`؛ وفرع ellipse في [exact.rs:58](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-geom/src/exact.rs:58). نحو **160–190 سطر معني**.

    **الوظيفة:** تحديد عينات المنحنيات وتقريب طول ellipse بتكامل Simpson أو معادلة Ramanujan.

    **البديل:** APIs الموجودة بالفعل في revision المثبّت من `opencadkernel`: `Curve::tessellate_within` و`Curve/EllipseArc::length`؛ **MPL-2.0، المصدر موجود في Cargo git checkout**.

    **المكسب والمخاطرة:** إزالة تنفيذ موازٍ وتوحيد السلوك. **مش ضمان لتحسين الدقة:** الكود الموجود عندنا يفحص quarter points للـNURBS، والـkernel له sampling caps وتكامل بعدد panels ثابت. لازم مقارنة منحنيات صعبة ووحدات مختلفة قبل النقل؛ عشان كده التوصية later.

18. **فهارس spatial grid إضافية مكتوبة يدويًا: replace now؛ medium.**

    **المكان:** [check.rs:179](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/src/check.rs:179): `Grid::new/within/near`؛ [ink.rs:254](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/src/ink.rs:254): `SheetInk::cell_range/index/near`؛ نحو **150 سطر**.

    **الوظيفة:** توزيع العناصر على خلايا، تكرار IDs عبر الخلايا، وجمع مرشحي الاستعلام المكاني.

    **البديل:** `rstar` 0.12.2، **MIT/Apache-2.0، موجود**.

    **المكسب والمخاطرة:** تقليل bookkeeping ومسارات التعامل الخاصة بالعناصر الكبيرة؛ مش اقتراح إعادة بناء الـ spatial index المستخدم بالفعل في مكان آخر. يُحتفظ بالفحص الهندسي الدقيق بعد broad phase، وتوسيع AABBs بالـ tolerance، وترتيب النتائج deterministically. تحسن السرعة أو الذاكرة يحتاج قياسًا؛ لم أفترضه.

19. **خوادم HTTP مصغرة داخل الاختبارات: replace later؛ medium.**

    **المكان:** [tests.rs:435](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-ai/src/chatgpt_plan/tests.rs:435): `mock_server`؛ [oauth.rs:823](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-ai/src/chatgpt_plan/oauth.rs:823): mock token/JWKS server؛ [settings.rs:443](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-desktop/tests/settings.rs:443). نحو **100–140 سطر plumbing**.

    **الوظيفة:** قراءة HTTP من socket، تفسير Content-Length، وصياغة responses يدويًا.

    **البديل:** `wiremock`: **MIT/Apache-2.0 حسب المعروف؛ غير موجود، verify**.

    **المكسب والمخاطرة:** assertions على requests وخادم HTTP صحيح؛ بعض الاختبارات الحالية تفترض كفاية `read` واحدة. نحتفظ بالـ raw sockets للاختبارات اللي هدفها تحديدًا قطع الاتصال أو تشويه البروتوكول، وننقل mocks العادية فقط.

20. **Roving focus للتبويبات والـ toolbar: replace now؛ medium.**

    **المكان:** [useRoving.ts:1](/home/kareem/Work/qs-mind/qiyas/src/shell/useRoving.ts:1): `nextIndex`, `useRoving`, `useToolbarRoving`، نحو **89 سطر**؛ مستخدمة في `FileTabs`, `SheetTabs`, `Ribbon`.

    **الوظيفة:** تعديل tabIndex يدويًا، التنقل بالأسهم، وإدارة التركيز مع تغيّر العناصر وRTL.

    **البديل:** `@base-ui/react` 1.9، **MIT، موجود**؛ `Tabs` و`Toolbar`.

    **المكسب والمخاطرة:** إزالة DOM querying وإدارة التركيز اليدوية والاستفادة من primitives متخصصة. لازم ضبط manual activation للتبويبات، وعدم تغيير اختصارات viewport أو سلوك غلق الملفات وsplit buttons.

21. **فتح وإغلاق وتموضع قوائم Recents وViews: replace now؛ medium.**

    **المكان:** [useMenu.ts:1](/home/kareem/Work/qs-mind/qiyas/src/shell/useMenu.ts:1)، [RecentsMenu.tsx:22](/home/kareem/Work/qs-mind/qiyas/src/shell/RecentsMenu.tsx:22)، [ViewsMenu.tsx:23](/home/kareem/Work/qs-mind/qiyas/src/shell/ViewsMenu.tsx:23)؛ نحو **70–100 سطر سلوك عام**.

    **الوظيفة:** outside click وEscape وحالة الفتح وقياس موضع popup يدويًا.

    **البديل:** Base UI `Menu`/`Popover`، **MIT، موجود**.

    **المكسب والمخاطرة:** focus return، keyboard navigation، وpositioning مع حدود الشاشة. القائمة اللي فيها form أو controls متعددة قد تحتاج `Popover` بدل تحويلها قسرًا إلى ARIA menu. تحميل البيانات وعمليات الحفظ والحذف تفضل كما هي.

22. **كاتب PDF وRunLength encoder داخل مولّد ملفات الـ benchmark: replace now؛ small.**

    **المكان:** [pdf.rs:54](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-cli/src/bench/generate/pdf.rs:54): فرع scan في `write`، و`pdf_stream`, `run_length`؛ نحو **85 سطر**.

    **الوظيفة:** كتابة PDF objects وxref وtrailer وضغط pixels بصيغة RunLength يدويًا.

    **البديل:** `lopdf` 0.44: **MIT، موجود**، أو `pdf-writer`: **MIT/Apache-2.0، موجود**؛ وضغط `flate2` الموجود: **MIT/Apache-2.0**.

    **المكسب والمخاطرة:** إزالة مسؤولية تنسيق PDF وencoder إضافي. هتتغير bytes وحجم الملف، لكن لازم يفضل scan بلا text layer وبنفس الأبعاد. ده موضع يدوي مستقل رغم استخدام lopdf في المنتج.

23. **تكرار تنفيذ FNV-1a وAdler-32: replace now؛ small.**

    **المكان:** [session.rs:2838](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/src/session.rs:2838): `Fnv`؛ `knowledge.rs:18`: `revision_fingerprint`؛ `read_revision.rs:110–135`؛ `underlay.rs:119`؛ `discrepancy/compare.rs:190`: `fnv1a`؛ `discrepancy/mod.rs:45`: `finding`؛ و`qiyas-desktop/src/ai.rs:267`: `provider_identity`. كذلك [ctb.rs:368](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-plot/src/ctb.rs:368): `adler32`. نحو **60–80 سطر مجمّعة**.

    **الوظيفة:** حساب خوارزميتين قياسيتين في تطبيقات متكررة.

    **البديل:** `fnv` 1.0.7: **MIT/Apache-2.0**؛ `adler2` 2.0.1 باختيار **MIT أو Apache-2.0**؛ **الاتنين موجودين**.

    **المكسب والمخاطرة:** إزالة النسخ اليدوية. لازم استخدام نفس seed وترتيب ونوع bytes؛ استخدام `Hash` بدل `Hasher::write` على نفس bytes ممكن يغيّر القيم. قواعد الهوية والبصمة نفسها تفضل كما هي.

24. **سلوك combobox/listbox في اختيار بند BOQ: replace now؛ medium.**

    **المكان:** [LinkPicker.tsx:20](/home/kareem/Work/qs-mind/qiyas/src/boq/LinkPicker.tsx:20): `LinkPicker`، نحو **60 سطر** من outside click، active item، keyboard handling، وscroll-to-item.

    **الوظيفة:** تنفيذ picker قابل للبحث والاختيار بالكيبورد.

    **البديل:** Base UI `Combobox` مع popup مناسب، **MIT، موجود**.

    **المكسب والمخاطرة:** إدارة التركيز وARIA والتفاعل من مكتبة واحدة. نحافظ على filtering الخاص بالـBOQ، العنصر الحالي، وعدم الربط، وترتيب النتائج؛ ونراجع opener focus وسلوك Escape.

25. **خوارزمية تعبئة glyph atlas: replace later؛ medium.**

    **المكان:** [atlas.rs:17](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-render/src/atlas.rs:17): `Shelf`, `Page::allocate`، واختيار الصفحة في `Atlas::insert` عند 100؛ نحو **60 سطر**.

    **الوظيفة:** وضع مستطيلات الحروف في shelves حسب ارتفاعها وفتح صفحات إضافية.

    **البديل:** `etagere` 0.2.15، **MIT/Apache-2.0، موجود**.

    **المكسب والمخاطرة:** allocator مخصص للـ texture atlases وفرصة لاستغلال المساحة أفضل؛ النتيجة الفعلية محتاجة قياس. لازم الحفاظ على padding وثبات UVs: مفيش repacking أو eviction يكسر batches قديمة. نسخ pixels ونسخ IPC/versioning تفضل عندنا.

26. **قارئ Server-Sent Events: replace now؛ medium.**

    **المكان:** [provider.rs:626](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-ai/src/provider.rs:626): `SseReader::push/next_event`، نحو **40 سطر**؛ تكامله في `read_events` عند 665.

    **الوظيفة:** تجميع chunks إلى SSE events واستخراج `data:`.

    **الدليل:** `push` يحذف كل CR ثم ينتظر LF/LF؛ بالتالي streams التي تستخدم CR وحده كفاصل سطور لا تُقرأ بالطريقة القياسية.

    **البديل:** `eventsource-stream`، **MIT/Apache-2.0 حسب المعروف؛ غير موجود، verify**.

    **المكسب والمخاطرة:** معالجة قواعد framing وUTF-8 وBOM والسطور من parser متخصص. يلزم adapter للـreqwest stream والحفاظ على cancellation ورسالة فشل الاتصال قبل الحدث النهائي. سياسات انتهاء رد النموذج وإعادة المحاولة تفضل عندنا.

27. **HTTP parser داخل OAuth loopback callback: replace now؛ medium.**

    **المكان:** [oauth.rs:202](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-ai/src/chatgpt_plan/oauth.rs:202): `loopback_code`، نحو **65 سطر**.

    **الوظيفة:** استقبال اتصال محلي، قراءة HTTP request، استخراج callback target وكتابة response.

    **البديل:** `axum` 0.8، **MIT، موجود ومستخدم بالفعل**.

    **المكسب والمخاطرة:** إزالة parser/server يدوي مستقل عن منطق OAuth. لازم الحفاظ على bind للـloopback، حدود الوقت والحجم، الإلغاء، وإغلاق الخادم بعد callback مناسب. `state` والتحقق من callback يفضلوا في طبقة البروتوكول؛ ده تغيير مستقل عن البند 14.

28. **متابعة آخر رسالة أثناء streaming: replace later؛ small.**

    **المكان:** [useStickToBottom.ts:1](/home/kareem/Work/qs-mind/qiyas/src/ai/useStickToBottom.ts:1): `useStickToBottom`، نحو **50 سطر**.

    **الوظيفة:** إبقاء المحادثة عند آخرها ما دام المستخدم ما طلعش لفوق.

    **الدليل:** المتابعة تعتمد على dependencies الممرّرة للـeffect؛ مفيش مراقبة مستقلة لتغيّر ارتفاع المحتوى بعد تحميل صورة أو خط.

    **البديل:** `use-stick-to-bottom`، **MIT حسب المعروف؛ غير موجود، verify للصيانة والإصدار والـAPI**.

    **المكسب والمخاطرة:** متابعة تغير حجم المحتوى وإدارة scrolling متخصصة. لازم الحفاظ على reduced motion، فتح سجل محادثة، وعدم سحب المستخدم للأسفل أثناء القراءة. later لأن المكسب محدود مقارنة بالهندسة والنص، ولأن المصدر مش متاح للتحقق هنا.

29. **CSV parser في عارض الملفات: replace now؛ small.**

    **المكان:** [FileViewer.tsx:243](/home/kareem/Work/qs-mind/qiyas/src/shell/FileViewer.tsx:243): `parseCsv`، نحو **18 سطر**.

    **الوظيفة:** قراءة quoted cells والفواصل والأسطر الجديدة بمكينة حالات صغيرة.

    **الدليل:** quote غير المغلقة لا تنتج خطأ، ومفيش معالجة صريحة للـBOM أو diagnostics.

    **البديل:** `papaparse`، **MIT حسب المعروف؛ غير موجود، verify للإصدار**.

    **المكسب والمخاطرة:** parser معروف مع أخطاء وتغطية edge cases. يجب تثبيت delimiter والسياسة المطلوبة للأسطر الفارغة، وتعطيل `dynamicTyping` للحفاظ على الأكواد والأصفار الأولى والأرقام الكبيرة كنص.

30. **استخراج التاريخ المحلي عبر libc يدويًا: replace now؛ small.**

    **المكان:** [field.rs:254](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/src/field.rs:254): `local_today`، نحو **24 سطر**.

    **الوظيفة:** استدعاء `time/localtime_r` على Unix والرجوع إلى UTC على الأنظمة الأخرى.

    **البديل:** `chrono::Local`، **MIT/Apache-2.0، المصدر 0.4.45 موجود** ومستخدم في crates أخرى.

    **المكسب والمخاطرة:** إزالة unsafe/FFI ومعالجة التاريخ المحلي على Windows أيضًا؛ الفرق يظهر قرب منتصف الليل. `today()` المعرّفة صراحة كتاريخ UTC تفضل UTC. إضافة dependency للـcore لها تكلفة حجم بسيطة تُراجع عند التنفيذ.

31. **خوارزمية Levenshtein لاقتراح أسماء الأوامر: replace now؛ small.**

    **المكان:** [surface.rs:1973](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-ai/src/surface.rs:1973): `close_commands::distance`، نحو **16 سطر**.

    **الوظيفة:** حساب edit distance بصف dynamic programming مكتوب يدويًا.

    **البديل:** `strsim` 0.11.1، **MIT، موجود**؛ `levenshtein`.

    **المكسب والمخاطرة:** حذف تنفيذ قياسي جاهز، مع مخاطرة منخفضة لو احتفظنا بالمقارنة على Unicode scalar values وبنفس thresholds والترتيب. لا يغيّر قواعد fuzzy matching أو تطبيع أوصاف BOQ.

32. **JSON diff يدوي لتشخيص فشل اختبارات الاشتقاق: replace now؛ small.**

    **المكان:** [derived_tests.rs:30](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/src/derived_tests.rs:30): `differences` داخل `equivalent`، نحو **30 سطر**.

    **الوظيفة:** المشي على objects وarrays لإظهار الفرق بين incremental derivation وإعادة الاشتقاق الكامل.

    **الدليل:** استبدال المفتاح الغائب بـ`Value::Null` قد يخفي فرق missing-versus-null في رسالة التشخيص؛ الاختبار نفسه يظل يفشل بسبب المقارنة الخارجية.

    **البديل:** `json-patch` 4.2، **MIT/Apache-2.0، موجود**؛ `diff`.

    **المكسب والمخاطرة:** diagnostics تميّز الإضافة والحذف وnull وتعالج JSON Pointer escaping. شكل رسالة الفشل هيتغير؛ المقارنة المستقلة بين الاشتقاقين ومنطق الاختبار يفضلوا كما هما.

**الملخص:** 32 finding؛ **23 replace now** و**9 replace later**. أعلى خمس أولويات: **الـ robust predicates والاحتواء، تفكيك التقاطع الذاتي للمساحة، ملكية الثقوب بعد booleans، تقريب Bézier في PDF، وUnicode line breaking**.

استبعدت المكتبات والتغييرات المحسومة، OCR، قواعد المنتج، DotLoader والـpanel rails؛ وكذلك الأجزاء الصغيرة اللي لم يظهر لاستبدالها مكسب كافٍ، زي caches محدودة الحجم، سياسة retry الحالية، وتحويل الأرقام العربية المحدد لاحتياج المنتج.