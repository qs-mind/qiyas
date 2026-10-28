# استبدال مكتبات الهندسة في قياس: 28 أكتوبر 2026

التنفيذ مقتصر على البنود 1 و2 و3 و4 و18 من المراجعة. الكود متروك من غير commit. مفيش أي compilation أو build أو تشغيل اختبارات؛ تطابق كل الأرقام مع الاختبارات الحالية لسه غير مثبت بالتشغيل.

## 1. إيه اتغير وليه

| بند المراجعة | التنفيذ |
|---|---|
| 1 | `qiyas-geom/src/predicates.rs` هو المكان المشترك الوحيد للتقاطع، الاحتواء، سماحية الحافة، orientation، المسافة للقطعة، hull، مساحة الحلقة ومركزها. `geo` بيحسب الهندسة و`robust` بيحسب orientation؛ النسخ القديمة اتشالت من الملفات السبعة في core ومن walls. أسماء الاستدعاءات الخارجية اتفضلت عن طريق re-export، أو اتحدثت الاستدعاءات. |
| 2 | Clipper64 مع `FillRule::EvenOdd` و`PolyTree64` بدل `split_loops/first_crossing/segment_hit` والـshoelace اليدوي. المسار البسيط بياخد مساحة geo بدون quantization؛ المسار المتقاطع بيتحل مرة لوجوه وثقوب. `Path::loops` بقى بيرجع `Result`، واستدعاؤه الوحيد في الإنتاج، قياس الهاتش، اتحدث. |
| 3 | `union_into` و`cut_from` بيحولوا المناطق وحفرها لـClipper ويرجعوا ملكية الثقوب والجزر من الشجرة. مفيش توزيع ثقوب حسب أول vertex. ترشيح المناطق اللي حدودها ممكن تتلامس فضل موجود، عشان كل تعديل ما يعيدش قص مناطق بعيدة. أخطاء القص بترجع `WallError`. |
| 4 | `kurbo::flatten` بيستخدم `FLATNESS_PT = 0.1 * 72 / 25.4` الحالية بعد تحويل الكنترول بوينتس لفضاء الصفحة وقبل ضربها في scale الرسم. البداية مش بتتكرر، والنهاية بتحتفظ بقيمتها الأصلية. اتشال سقف 1000 قطعة اللي كان ممكن يكسر الدقة. فيه رفض صريح للمنحنيات غير المحدودة أو الأكبر من ميزانية الموارد، بدل تغيير السماحية. |
| 18 | شبكتا `check.rs` و`ink.rs` بقوا R-trees من rstar الموجود أصلًا. صناديق البحث بتتوسع بسماحية المستدعي و`tol_near`؛ IDs بتترتب حسب ترتيب المصدر، والفحص الهندسي الدقيق فضل بعد البحث. قوائم العناصر الكبيرة وتكرار IDs بين الخلايا اتشالت. |

الوحدات لسه drawing units. `measure_tol` و`SAME_POINT` و`tol_near` ما اتغيروش. تطبيع الحوائط بالأصل المحلي والسمك فضل موجود. مساحة ومركز الحلقة، والمسافات والتقاطعات، وتقريب PDF بيستخدموا أصل محلي. `robust::orient2d` بياخد إحداثيات الإدخال الأصلية عشان يحافظ على bits اللي الـadaptive predicate محتاجها.

قواعد الحافة ما اتوحدتش قسرًا: الاحتواء القديم half-open فضل زي ما هو، الاختيار شامل الحافة بسماحيته النسبية، وmeasurement scope شامل مسافة السماحية المطلقة. `geo` بيستخدم nonzero winding للحلقة الكاملة، لذلك adapter الاحتواء بيستخدم XOR لاختبارات مثلثات geo، ومع الحافة أو خط فصل المثلثات بيستخدم تقاطعات geo مع قاعدة الشعاع half-open القديمة. دي سياسة even-odd مشتركة، مش نسخة ثانية من determinants أو ray-coordinate arithmetic القديمة.

التحويل لـClipper بقوة اتنين، بأصل محلي، وinteger magnitude لحد 2^50. الدقة المستهدفة 2^-40 من وحدة الإدخال أو أدق للمناطق الصغيرة، مع تخفيض المقياس للامتدادات الكبيرة. رؤوس الإدخال اللي رجعت بنفس عنوانها الصحيح بتسترد إحداثياتها الأصلية، عشان المقارنات التامة الحالية تفضل ممكنة. نقاط التقاطع الجديدة فقط بتتحول من شبكة Clipper.

التبعيات الأربعة مثبتة على `geo =0.33.1` و`robust =1.2.0` و`clipper2-rust =1.2.0` و`kurbo =0.13.1` بنمط workspace. geo بدون default features: مفيش earcut أو spade أو multithreading. kurbo بميزة std فقط. rstar اتضاف كاعتماد مباشر لـcore من تعريف workspace الموجود. ملف BSL-1.0 وصف clipper في THIRD-PARTY كانوا موجودين، فما اتكتبوش فوقهم؛ اتضاف قسم الهندسة للمكتبات الثلاثة الباقية.

البندان 11 و17 متسابين: offsets وmitre وقواعد السمك وتقاطعات الوصلات الخاصة بالحوائط، وkernel المنحنيات، ما اتبدلوش. `regions_overlap` فضل فوق Cavalier الموجود بالفعل؛ ده استدعاء مكتبة قائم، مش bookkeeping الثقوب اللي البند 3 طلب استبداله.

## 2. الملفات والسطور المحذوفة

الأرقام مقارنة بنسخة الشجرة عند بدء المهمة، مش بـHEAD، عشان ما تتحسبش تعديلات المهندس التاني. «كود محذوف» هنا عدد سطور diff المحذوفة بعد استبعاد الفراغ والتعليقات `//`؛ الرقم بيشمل السطور اللي اتنقلت أو اتبدلت، ومش ادعاء بصافي تقليل حجم المشروع.

| الملف | سطور محذوفة إجمالًا | منها كود | سطور مضافة |
|---|---:|---:|---:|
| [Cargo.toml](/home/kareem/Work/qs-mind/qiyas/Cargo.toml) | 0 | 0 | 5 |
| [LICENSES/THIRD-PARTY.md](/home/kareem/Work/qs-mind/qiyas/LICENSES/THIRD-PARTY.md) | 0 | 0 | 12 |
| [crates/qiyas-geom/Cargo.toml](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-geom/Cargo.toml) | 0 | 0 | 3 |
| [crates/qiyas-geom/src/lib.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-geom/src/lib.rs) | 0 | 0 | 2 |
| [crates/qiyas-geom/src/path.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-geom/src/path.rs) | 80 | 64 | 32 |
| [crates/qiyas-geom/src/walls.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-geom/src/walls.rs) | 194 | 180 | 36 |
| [crates/qiyas-pdf/Cargo.toml](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-pdf/Cargo.toml) | 0 | 0 | 1 |
| [crates/qiyas-pdf/src/vectors.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-pdf/src/vectors.rs) | 34 | 33 | 73 |
| [crates/qiyas-core/Cargo.toml](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/Cargo.toml) | 0 | 0 | 1 |
| [crates/qiyas-core/tests/all.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/tests/all.rs) | 0 | 0 | 3 |
| [crates/qiyas-core/src/measurement_scope.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/src/measurement_scope.rs) | 54 | 53 | 3 |
| [crates/qiyas-core/src/session.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/src/session.rs) | 66 | 54 | 5 |
| [crates/qiyas-core/src/ink.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/src/ink.rs) | 69 | 69 | 25 |
| [crates/qiyas-core/src/check.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/src/check.rs) | 114 | 104 | 37 |
| [crates/qiyas-core/src/draw.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/src/draw.rs) | 23 | 23 | 4 |
| [crates/qiyas-core/src/room_objects.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/src/room_objects.rs) | 14 | 14 | 3 |
| [crates/qiyas-core/src/element_metadata.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/src/element_metadata.rs) | 34 | 33 | 4 |
| [crates/qiyas-geom/src/predicates.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-geom/src/predicates.rs)، جديد | 0 | 0 | 218 |
| [crates/qiyas-geom/src/topology.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-geom/src/topology.rs)، جديد | 0 | 0 | 119 |
| [crates/qiyas-core/tests/geometry_libraries.rs](/home/kareem/Work/qs-mind/qiyas/crates/qiyas-core/tests/geometry_libraries.rs)، جديد | 0 | 0 | 244 |

الإجمالي المحذوف: **682 سطرًا، منها 627 سطر كود** بالتعريف أعلاه. ملف التقرير ده جديد كمان. الشغل السابق في manifests وroom_objects وTHIRD-PARTY وtests/all.rs اتساب، والتعديل اقتصر على نطاق المهمة. ملف BSL الموجود وCargo.lock ما اتعدلوش.

## 3. نتيجة كل فحص

| الفحص | النتيجة الحقيقية |
|---|---|
| cargo بأي subcommand، compilation، build، clippy، Rust tests | لم يُشغَّل أي منها، حسب التعليمات. |
| pnpm، tsc، vitest وأي اختبارات أخرى | لم يُشغَّل أي منها. |
| اختبارات الدقة والأداء الحالية | لم تُشغَّل؛ لا يوجد ادعاء بالنجاح أو تطابق النتائج. |
| الاختبارات التسعة الجديدة في core/tests/geometry_libraries.rs | مكتوبة ومسجلة في tests/all.rs داخل نفس test binary؛ لم تُشغَّل. |
| standalone rustfmt على الملفات المعدلة فقط | اشتغل مرة، exit 0. `skip_children=true` و`reorder_modules=false` لتجنب لمس ملفات الاختبارات والوحدات الأخرى. |
| قراءة diff والمصادر المحلية | اتراجعت ساكنًا. مقارنة نصية أكدت إن أجسام الاختبارات الموجودة في walls.rs وvectors.rs ما اتغيرتش. دي مش compilation أو تحقق عددي. |
| شبكة، نافذة، browser، git add/commit/push | مفيش أي واحد منهم اتعمل. |

الاختبارات الجديدة بتغطي half-open boundaries في الاتجاهين وعند survey coordinates، سماحيات الحافة، المسار المكرر والـbowtie، hole/island متصلين بجسور مرتجعة، collinear/degenerate/tiny-determinant intersections، segment containment داخل شكل مقعر، hull/area/centroid، حائط داخل ثقب غرفة وفتحة باب، ومرشحي rstar عند السماحية وترتيب التقرير في أربع وحدات.

## 4. اختلافات النتائج المحتملة والحدود الباقية

الحالات التالية مستنتجة من الكود وواجهات المكتبات، وليست نتائج تشغيل:

1. **مسار بيكرر نفس المربع مرتين:** مربع 10×10 كان ممكن يتحسب 200 بجمع اللوبس؛ even-odd الجديد بيدي صفر. الـbowtie العادي لسه مستهدف يحسب مجموع الفصين، 2 للمثال الموجود.
2. **ثقب متصل بحد خارجي بجسر رايح وراجع، ومعاه جزيرة داخل الثقب:** `Path::area` كان بيجمع مساحات اللوبس؛ الجديد بيطرح الثقب ويضيف الجزيرة. المثال الجديد 100 - 36 + 4 = 68. الهاتش كان مقصوده even-odd أصلًا، واستمر بنفس السياسة وبالمساحات exact للمسارات البسيطة ذات الـbulges.
3. **حواف متراكبة على نفس الخط، أو قطع منعدمة الطول متلامسة:** `self_intersects` وink intersection بيكتشفوها دلوقتي. ده يصلح استبعاد التوازي الكامل القديم، وممكن يضيف collision أو علامة self-intersecting، أو يمنع استخدام outline متراكب كـview-only region. proper crossing لسه بيستبعد مجرد اللمس؛ `crossing` لسه بيرجع None لما التقاطع interval مش نقطة وحيدة.
4. **تقاطع صغير المحدد:** مثل `[0,0]..[1,1e-16]` مع `[0,1e-16]..[1,0]`؛ شرط `abs(den) < 1e-15` كان بيضيعه في check/ink. الجديد بيكشف نقطة `[0.5,5e-17]`. orientation قرب collinearity والحساب قرب survey coordinates ممكن يصحح قرار اتأثر بتقريب determinant أو إلغاء أرقام كبيرة. مفيش تغيير متعمد لتعريف half-open أو سماحيات الحافة.
5. **قص داخلي بالكامل من منطقة:** مثال منطقة 10×10 وcut داخلي 2×2؛ الكود القديم كان بيتجاهل neg_plines من أول Difference وممكن يسيب المساحة 100 بدل 96. الجديد بيرجع الثقب من PolyTree. انقسام المنطقة، اتصال الثقب بالخارج، اتحاد مادة داخل ثقب، وملكية الجزر دلوقتي كلها من نفس الشجرة. المثال الداخلي ده متراجع ساكنًا؛ مفيش تشغيل مستقل يثبته.
6. **ثقوب صغيرة:** union القديم كان بيسقط hole area <= 1e-12. الحد المنفصل ده اتشال؛ Clipper بيحتفظ بالثقب لو شبكة الدقة بتقدر تمثله. مفيش ادعاء إن تفاصيل أصغر من الشبكة هتفضل موجودة.
7. **ترتيب vertices واللوبس والأجزاء، وwinding الثقوب:** ممكن يختلف عن ترتيب Cavalier أو recursive splitting القديم، لكنه حتمي. الحلقات الناتجة بتبدأ من أصغر نقطة lexicographically، والثقوب والأجزاء الناتجة من نفس العملية بتترتب. اتجاه outer موجب والثقوب سالب من Clipper. طول حد الحلقة وعدد vertices ممكن يختلف في الحالات المتلامسة والمتراكبة.
8. **الدقة العددية:** المساحة والمركز بأصل محلي بتصلح cancellation في `draw::twice_area` ومساحات paths البعيدة. نقاط التقاطع الجديدة في Clipper لها quantization؛ حتى لو خطوات الشبكة أصغر من سماحيات النقاط الحالية، ده مش برهان إن كل اختبار مساحة tight هينجح. المقارنة الفعلية المطلوبة ما حصلتش. كذلك تقريب geo للمسافة ممكن يغير قرار عند الحد العددي للسماحية.
9. **PDF:** عدد القطع ومواقع العينات وبالتالي المساحة والطول المقاسان من polyline ممكن يختلفوا مع نفس سماحية 0.1 مم على الصفحة. عند تجاوز سقف 1000 القديم، الدقة دلوقتي ما بتتخففش. المدخلات غير finite أو control extent أكبر من `FLATNESS_PT * 1e8` نقطة، أو خرج أكبر من 65,536 نقطة، بيرجعوا `PdfError::Page` بدل استيراد تقريب غير مضمون. حد extent ده قيد موارد اختير هنا، ومش benchmark متقاس؛ الحد لكل cubic، مش ميزانية إجمالية للصفحة.
10. **الفهرس المكاني:** توسيع AABB ممكن يسترجع overlap داخل السماحية كانت الخلايا الدقيقة بتفوّته عند حد خلية. الفحص الدقيق نفسه فضل موجود، وترتيب المرشحين حسب IDs الأصلية. لا يوجد قياس لسرعة rstar أو الذاكرة.
11. **فشل القص:** walls بيرجع error؛ `Path::loops` بقى `Result`، والـarea/قياس الهاتش بيطلع NaN عند فشل clipping بدل تحويل الفشل لمساحة صفر. الاستدعاء الداخلي اتحدث؛ أي مستهلك خارجي غير موجود في الشجرة هيحتاج يتعامل مع نوع الرجوع الجديد.

المتبقي للتحقق، مش تغيير إضافي اتنفذ: تشغيل check المسموح به لاحقًا، وحل Cargo.lock، ومقارنة الدقة والأداء. أرقام نجاح الاختبارات وتوافق كل الاعتمادات الانتقالية مش معروفة من غير Cargo. مصادر الـAPIs نفسها اتقرت، مفيش API اتكتب من الذاكرة وحدها. نضج وصيانة Rust port وadvisories ما اتراجعوش على الإنترنت لأن الشبكة ممنوعة. البحث عن self-intersection لسه بيفحص أزواج القطع O(n²)؛ اللي اتشال هو recursive splitting اليدوي، ومفيش ادعاء بتحسين تعقيد الكشف نفسه.

## مراجع APIs المحلية

كل المسارات تحت `/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`. الأرقام للنسخ المحلية المقروءة، ومفيش تنزيلات. أنواع geo الأساسية متصدرة من geo-types؛ النسخة المحلية المقروءة 0.7.20، بينما manifest geo يسمح بـ0.7.19 وما بعدها المتوافق. اختيار النسخة النهائي معلق لحل lockfile.

| API مستخدم | المصدر المحلي |
|---|---|
| geo: CoordinatePosition::coordinate_position وCoordPos، وتطبيق Triangle | [geo-0.33.1/src/algorithm/coordinate_position.rs:11](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/geo-0.33.1/src/algorithm/coordinate_position.rs:11), [geo-0.33.1/src/algorithm/coordinate_position.rs:36](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/geo-0.33.1/src/algorithm/coordinate_position.rs:36), [geo-0.33.1/src/algorithm/coordinate_position.rs:184](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/geo-0.33.1/src/algorithm/coordinate_position.rs:184) |
| geo: line_intersection، LineIntersection، is_proper | [geo-0.33.1/src/algorithm/line_intersection.rs:8](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/geo-0.33.1/src/algorithm/line_intersection.rs:8), [geo-0.33.1/src/algorithm/line_intersection.rs:34](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/geo-0.33.1/src/algorithm/line_intersection.rs:34), [geo-0.33.1/src/algorithm/line_intersection.rs:77](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/geo-0.33.1/src/algorithm/line_intersection.rs:77) |
| geo: Euclidean.distance(Coord, &Line) | [geo-0.33.1/src/algorithm/line_measures/metric_spaces/euclidean/distance.rs:38](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/geo-0.33.1/src/algorithm/line_measures/metric_spaces/euclidean/distance.rs:38) |
| geo: Area::signed_area، تطبيق Polygon | [geo-0.33.1/src/algorithm/area.rs:80](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/geo-0.33.1/src/algorithm/area.rs:80), [geo-0.33.1/src/algorithm/area.rs:139](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/geo-0.33.1/src/algorithm/area.rs:139) |
| geo: Centroid::centroid، تطبيق Polygon | [geo-0.33.1/src/algorithm/centroid.rs:36](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/geo-0.33.1/src/algorithm/centroid.rs:36), [geo-0.33.1/src/algorithm/centroid.rs:159](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/geo-0.33.1/src/algorithm/centroid.rs:159) |
| geo: ConvexHull::convex_hull | [geo-0.33.1/src/algorithm/convex_hull/mod.rs:42](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/geo-0.33.1/src/algorithm/convex_hull/mod.rs:42), [geo-0.33.1/src/algorithm/convex_hull/mod.rs:58](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/geo-0.33.1/src/algorithm/convex_hull/mod.rs:58) |
| geo types: Coord وحقول x/y | [geo-types-0.7.20/src/geometry/coord.rs:25](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/geo-types-0.7.20/src/geometry/coord.rs:25) |
| geo types: Line::new وstart/end | [geo-types-0.7.20/src/geometry/line.rs:30](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/geo-types-0.7.20/src/geometry/line.rs:30) |
| geo types: LineString::new وبيانات الحلقة | [geo-types-0.7.20/src/geometry/line_string.rs:135](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/geo-types-0.7.20/src/geometry/line_string.rs:135), [geo-types-0.7.20/src/geometry/line_string.rs:195](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/geo-types-0.7.20/src/geometry/line_string.rs:195) |
| geo types: Polygon::new وexterior | [geo-types-0.7.20/src/geometry/polygon.rs:122](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/geo-types-0.7.20/src/geometry/polygon.rs:122), [geo-types-0.7.20/src/geometry/polygon.rs:192](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/geo-types-0.7.20/src/geometry/polygon.rs:192) |
| geo types: Point::x وPoint::y | [geo-types-0.7.20/src/geometry/point.rs:111](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/geo-types-0.7.20/src/geometry/point.rs:111), [geo-types-0.7.20/src/geometry/point.rs:168](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/geo-types-0.7.20/src/geometry/point.rs:168) |
| geo types: Triangle::unchecked_winding؛ اختبار الاحتواء لا يعتمد على اتجاه محدد | [geo-types-0.7.20/src/geometry/triangle.rs:55](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/geo-types-0.7.20/src/geometry/triangle.rs:55) |
| robust: Coord، orient2d | [robust-1.2.0/src/lib.rs:38](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/robust-1.2.0/src/lib.rs:38), [robust-1.2.0/src/lib.rs:77](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/robust-1.2.0/src/lib.rs:77) |
| Clipper: FillRule::EvenOdd، Point64::new، aliases Path64/Paths64 | [clipper2-rust-1.2.0/src/core.rs:13](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/clipper2-rust-1.2.0/src/core.rs:13), [clipper2-rust-1.2.0/src/core.rs:159](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/clipper2-rust-1.2.0/src/core.rs:159), [clipper2-rust-1.2.0/src/core.rs:513](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/clipper2-rust-1.2.0/src/core.rs:513), [clipper2-rust-1.2.0/src/core.rs:520](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/clipper2-rust-1.2.0/src/core.rs:520), [clipper2-rust-1.2.0/src/core.rs:525](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/clipper2-rust-1.2.0/src/core.rs:525) |
| Clipper: ClipType::Union/Difference | [clipper2-rust-1.2.0/src/engine.rs:25](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/clipper2-rust-1.2.0/src/engine.rs:25) |
| Clipper64: new، add_subject، add_clip، execute_tree والـbool للنجاح | [clipper2-rust-1.2.0/src/engine_public.rs:284](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/clipper2-rust-1.2.0/src/engine_public.rs:284), [clipper2-rust-1.2.0/src/engine_public.rs:298](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/clipper2-rust-1.2.0/src/engine_public.rs:298), [clipper2-rust-1.2.0/src/engine_public.rs:308](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/clipper2-rust-1.2.0/src/engine_public.rs:308), [clipper2-rust-1.2.0/src/engine_public.rs:363](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/clipper2-rust-1.2.0/src/engine_public.rs:363) |
| PolyTree64: new، nodes، is_hole؛ PolyPath64: polygon، children | [clipper2-rust-1.2.0/src/engine_public.rs:50](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/clipper2-rust-1.2.0/src/engine_public.rs:50), [clipper2-rust-1.2.0/src/engine_public.rs:62](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/clipper2-rust-1.2.0/src/engine_public.rs:62), [clipper2-rust-1.2.0/src/engine_public.rs:75](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/clipper2-rust-1.2.0/src/engine_public.rs:75), [clipper2-rust-1.2.0/src/engine_public.rs:80](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/clipper2-rust-1.2.0/src/engine_public.rs:80), [clipper2-rust-1.2.0/src/engine_public.rs:117](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/clipper2-rust-1.2.0/src/engine_public.rs:117) |
| Clipper: الافتراضي preserve_collinear=true | [clipper2-rust-1.2.0/src/engine.rs:418](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/clipper2-rust-1.2.0/src/engine.rs:418) |
| kurbo: flatten وPathEl::MoveTo/CurveTo/LineTo | [kurbo-0.13.1/src/bezpath.rs:114](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/kurbo-0.13.1/src/bezpath.rs:114), [kurbo-0.13.1/src/bezpath.rs:622](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/kurbo-0.13.1/src/bezpath.rs:622) |
| kurbo: Point::new وx/y | [kurbo-0.13.1/src/point.rs:28](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/kurbo-0.13.1/src/point.rs:28), [kurbo-0.13.1/src/point.rs:44](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/kurbo-0.13.1/src/point.rs:44) |
| rstar: RTree default، bulk_load، locate_in_envelope_intersecting | [rstar-0.12.2/src/rtree.rs:21](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rstar-0.12.2/src/rtree.rs:21), [rstar-0.12.2/src/rtree.rs:233](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rstar-0.12.2/src/rtree.rs:233), [rstar-0.12.2/src/rtree.rs:434](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rstar-0.12.2/src/rtree.rs:434) |
| rstar: GeomWithData::new وdata | [rstar-0.12.2/src/primitives/geom_with_data.rs:34](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rstar-0.12.2/src/primitives/geom_with_data.rs:34), [rstar-0.12.2/src/primitives/geom_with_data.rs:71](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rstar-0.12.2/src/primitives/geom_with_data.rs:71) |
| rstar: Rectangle::from_corners | [rstar-0.12.2/src/primitives/rectangle.rs:32](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rstar-0.12.2/src/primitives/rectangle.rs:32) |
| rstar: AABB::from_corners | [rstar-0.12.2/src/aabb.rs:60](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rstar-0.12.2/src/aabb.rs:60) |
| geo: default features والاعتمادات غير الاختيارية، ومنها i_overlay | [geo-0.33.1/Cargo.toml:37](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/geo-0.33.1/Cargo.toml:37), [geo-0.33.1/Cargo.toml:86](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/geo-0.33.1/Cargo.toml:86), [geo-0.33.1/Cargo.toml:101](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/geo-0.33.1/Cargo.toml:101) |
| kurbo: std feature فقط | [kurbo-0.13.1/Cargo.toml:42](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/kurbo-0.13.1/Cargo.toml:42), [kurbo-0.13.1/Cargo.toml:58](/home/kareem/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/kurbo-0.13.1/Cargo.toml:58) |
