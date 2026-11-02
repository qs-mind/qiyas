# Writing Arabic that reads native

Contents: Register (MSA vs dialect) · UI copy patterns · Translation traps · Punctuation · Numerals · Dates and calendars · Currency · Names, gender, plurals · Marketing, WhatsApp, social · Emails and letters

## Register

| Context | Use |
|---|---|
| UI, product, documentation, legal, reports, news, formal email | **Modern Standard Arabic (فصحى)**: simple, modern, short sentences |
| WhatsApp promos, social posts, ads for one market | MSA by default. Use the market's dialect only if asked or if the brand already speaks it |
| Chatbots, support replies | MSA, warm and direct. Mirror the user's dialect lightly if they write in it |
| Children's content, Quran/hadith quotes | MSA, with tashkeel where it helps reading |

Dialect markers, so you can tell what you're writing:

| Meaning | MSA | Egyptian | Gulf (Saudi/UAE) | Levantine |
|---|---|---|---|---|
| I want | أريد | عايز / عاوز | أبغى / أبي | بدي |
| now | الآن | دلوقتي | الحين | هلق / هلأ |
| what? | ماذا؟ | إيه؟ | وش؟ / إيش؟ | شو؟ |
| very | جدًا | أوي | مرة / وايد | كتير |
| don't miss | لا تفوّت | ما تفوّتش | لا يفوتك | لا تفوّت |

Never mix dialects in one text. If unsure of the market, write MSA.

## UI copy patterns

| ❌ Literal / awkward | ✅ Natural | Notes |
|---|---|---|
| انقر هنا للإرسال | أرسل | Buttons: imperative verb, 1–2 words |
| إرسال (on a button) | أرسل | Verbal noun (مصدر) is acceptable for menu items/labels (`الإعدادات`, `تسجيل الدخول`), but buttons read better as verbs |
| تسجيل الدخول إلى حسابك | سجّل الدخول | Keep it short |
| هل أنت متأكد أنك تريد الحذف؟ | هل تريد حذف هذا الملف؟ | Name the object, drop "are you sure" |
| خطأ: فشل في التحميل | تعذّر تحميل الملف. حاول مرة أخرى. | Say what happened and what to do |
| لا يوجد بيانات | لا توجد بيانات بعد | Agreement: بيانات is feminine plural |
| أهلا بك مرة أخرى! | أهلًا بعودتك! | Idiomatic |
| اضغط على الزر أدناه | اضغط الزر أدناه / اضغط على «متابعة» | Quote UI labels with «» |
| 5 عنصر | 5 عناصر | Count agreement (see plurals) |
| التالي → | التالي ← | Arrows point left for "next" |

Common UI vocabulary: حفظ/احفظ، إلغاء، متابعة، رجوع، التالي، السابق، إرسال، بحث، تصفية، الإعدادات، الملف الشخصي، تسجيل الدخول، إنشاء حساب، نسيت كلمة المرور؟، سلة التسوق، إتمام الشراء، الدفع، الشحن، تتبّع الطلب، الإشعارات، مساعدة، تواصل معنا، الشروط والأحكام، سياسة الخصوصية.

Address the user consistently. Masculine singular (`سجّل`، `اختر`) is the conventional default in UI. For mixed audiences consider neutral forms (verbal nouns `تسجيل`، passive, or `يمكنك…`). Don't write `سجل/ي` slash forms in UI unless the brand does.

## Translation traps
- **Word order:** Arabic is often verb-first. ❌ `المستخدم قام بتحديث الملف` ✅ `حدّث المستخدم الملف`
- **Drop the filler verbs** `قام بـ`، `تم + مصدر` when an active verb works: ❌ `تم إرسال الرسالة بنجاح` ✅ `أُرسلت الرسالة` (or `تم الإرسال` for very short toasts, which is acceptable)
- **Don't calque English idioms:** "in order to" ❌ `من أجل أن` → ✅ `لـ`; "at the end of the day" ❌ `في نهاية اليوم` → ✅ `في المحصلة`
- **Possessives:** ❌ `إعدادات الخاصة بك` ✅ `إعداداتك`
- **Capitalized English terms:** keep brand/product names in Latin (`iPhone`, `Claude`), and translate generic terms (`dashboard` → `لوحة التحكم`, `checkout` → `إتمام الشراء`). Don't transliterate when an established Arabic word exists.
- **Gendered agreement** follows the noun: `الصفحة الرئيسية` (f), `الملف الرئيسي` (m).
- **Dual** exists: `ملفان`، `يومان`. Don't write `2 ملفات`.
- **Hamza and taa marbuta matter:** `إلى` not `الى`, `مدرسة` not `مدرسه`, `مسؤول` (or `مسئول` in Egypt). Never swap ة/ه or ى/ي in final position. It's a common machine-text tell.

## Punctuation

| Mark | Arabic | Latin | Note |
|---|---|---|---|
| comma | `،` | `,` | Also in lists: `التفاح، والموز، والعنب` |
| semicolon | `؛` | `;` | |
| question mark | `؟` | `?` | |
| quotes | `«…»` | `"…"` | `"…"` is acceptable in technical text |
| period, exclamation, colon | `.` `!` `:` | same | |
| decimal / thousands (Arabic-Indic digits) | `٫` / `٬` | `.` / `,` | `١٬٢٣٤٫٥` |
| percent (Arabic-Indic digits) | `٪` | `%` | `٢٥٪` |

- No space before punctuation and one space after: ❌ `مرحبا ، كيف حالك ؟` ✅ `مرحبًا، كيف حالك؟`
- The conjunction `و` attaches to the next word: ❌ `و الموز` ✅ `والموز`
- Don't use Latin `,` `?` `;` inside Arabic sentences.
- Tanween on alif: `شكرًا`، `أهلًا`، `جدًا` (modern style puts the fatḥatān on the letter before the alif). `شكراً` is also common. Pick one and be consistent.

## Numerals

| Region | Typical everyday digits | Formal/print |
|---|---|---|
| Morocco, Algeria, Tunisia, Libya | `0-9` | `0-9` |
| UAE, Qatar, Bahrain, Kuwait, Oman, Lebanon, Jordan | mostly `0-9` in digital products | `٠-٩` common in print/government |
| Saudi Arabia, Egypt, Iraq, Syria, Sudan, Yemen | `0-9` in apps/web | `٠-٩` widely used in print, government, receipts |

- Digital products (apps, web, dashboards, prices): **Western digits by default** across the region. Use `٠-٩` when the user asks, the brand uses them, or the content is formal/editorial for Egypt or the Gulf.
- Never mix systems in one output. Phone numbers, codes, IBANs, card numbers: always Western digits in inputs, because users type them that way.
- Persian digits `۰-۹` (U+06F0…) are **different code points**. Never use them for Arabic: `۴ ۵ ۶` look different from `٤ ٥ ٦`.

`Intl` defaults differ by locale. Always set `nu` explicitly *(verified on ICU 77)*:
```js
new Intl.NumberFormat("ar").format(1234567.5)          // "1,234,567.5"   (ar alone = Latin digits)
new Intl.NumberFormat("ar-SA").format(1234567.5)       // "١٬٢٣٤٬٥٦٧٫٥"
new Intl.NumberFormat("ar-EG").format(1234567.5)       // "١٬٢٣٤٬٥٦٧٫٥"
new Intl.NumberFormat("ar-AE").format(1234567.5)       // "1,234,567.5"
new Intl.NumberFormat("ar-MA").format(1234567.5)       // "1.234.567,5"   (Maghreb separators)
new Intl.NumberFormat("ar-SA-u-nu-latn").format(1234567.5)  // "1,234,567.5"  force Western
new Intl.NumberFormat("ar-AE-u-nu-arab").format(1234567.5)  // force Arabic-Indic
```
❌ `new Intl.NumberFormat("ar-SA")` and assuming Western digits
✅ `new Intl.NumberFormat("ar-SA-u-nu-latn")` or `"ar-SA-u-nu-arab"`, whichever you decided

Python: `str.translate(str.maketrans("0123456789", "٠١٢٣٤٥٦٧٨٩"))` for display only. `babel.numbers.format_decimal(1234.5, locale="ar_EG")` if Babel is available.

## Dates and calendars
```js
const d = new Date(Date.UTC(2026, 9, 18));
new Intl.DateTimeFormat("ar-EG-u-nu-latn", { dateStyle: "long", timeZone: "UTC" }).format(d)  // "18 أكتوبر 2026"
new Intl.DateTimeFormat("ar-JO", { dateStyle: "long" }).format(d)          // "١٨ تشرين الأول ٢٠٢٦" (Levantine month names)
new Intl.DateTimeFormat("ar-SA-u-ca-gregory-nu-latn", { dateStyle: "long" }).format(d)        // "18 أكتوبر 2026"
new Intl.DateTimeFormat("ar-SA-u-ca-islamic-umalqura-nu-latn", { dateStyle: "long" }).format(d) // "7 جمادى الأولى 1448 هـ"
new Intl.RelativeTimeFormat("ar", { numeric: "auto" }).format(-1, "day")  // "أمس"
```
*(outputs verified on Node 22 / ICU 77)*
- **`ar-SA`'s default calendar differs across ICU versions and browsers** (Gregorian on ICU 77, Hijri in some older engines). Always pass `-u-ca-gregory` or `-u-ca-islamic-umalqura`.
- Month names: Egypt/Gulf/Maghreb use `يناير، فبراير…` (Maghreb: `يناير، فبراير، مارس، أبريل، ماي، يونيو، يوليوز…` in Morocco). Levant/Iraq use `كانون الثاني، شباط، آذار…`. Let `Intl` pick by region.
- Hijri (`islamic-umalqura`) for Saudi government, religious dates, Ramadan/Eid campaigns. Show both when useful: `18 أكتوبر 2026 (7 جمادى الأولى 1448 هـ)`.
- First day of week and weekend differ by country: Saudi Arabia starts Sunday with a Friday–Saturday weekend, the UAE starts Monday with a Saturday–Sunday weekend *(ICU 77)*. Read them from `new Intl.Locale("ar-SA").getWeekInfo()` (or `.weekInfo`) instead of hard-coding.
- Time: `3:30 م` (مساءً) / `9:00 ص` (صباحًا). The 12-hour clock is common in casual UI and 24-hour in transport/official contexts.

## Currency

| Country | Code | Arabic symbol / name | Example |
|---|---|---|---|
| Saudi Arabia | SAR | ر.س / ريال (the new riyal sign U+20C1, Unicode 17, is missing from most fonts, so don't rely on it) | `49.99 ر.س` |
| UAE | AED | د.إ / درهم | `49.99 د.إ` |
| Egypt | EGP | ج.م / جنيه | `49.99 ج.م` |
| Kuwait | KWD | د.ك / دينار (3 decimals) | `4.500 د.ك` |
| Qatar | QAR | ر.ق | `49.99 ر.ق` |
| Jordan | JOD | د.أ (3 decimals) | `4.500 د.أ` |
| Morocco | MAD | د.م. / درهم | `49,99 د.م.` |

```js
new Intl.NumberFormat("ar-AE", { style: "currency", currency: "AED" }).format(49.99)        // "49.99 د.إ.‏"
new Intl.NumberFormat("ar-EG", { style: "currency", currency: "EGP" }).format(49.99)        // "٤٩٫٩٩ ج.م.‏"
new Intl.NumberFormat("ar-SA-u-nu-latn", { style: "currency", currency: "SAR" }).format(49.99)
```
- The Arabic symbol goes **after** the number in Arabic text. `$49.99` / `US$` stays as is.
- `Intl` output contains invisible RLM/ALM marks. Don't strip them, and don't compare formatted strings in tests.
- In copy, a word often reads better than an abbreviation: `49 ريالًا`، `100 درهم`.

## Names, gender, plurals
- Arabic plural rules have six categories *(verified with `Intl.PluralRules("ar")`)*: 0, 1, 2, 3–10, 11–99, 100–102 (and every n whose last two digits are 00–02). Use ICU plurals (`Intl.PluralRules("ar")`, i18next/FormatJS plural keys `zero one two few many other`) instead of `count + " عناصر"`.

| n | Form | Example |
|---|---|---|
| 0 | zero | لا توجد عناصر |
| 1 | one | عنصر واحد |
| 2 | two | عنصران |
| 3–10 | few | 3 عناصر |
| 11–99 | many | 11 عنصرًا |
| 100+ | other | 100 عنصر |

- Gender: verbs and adjectives agree with the subject. When addressing the user, keep one convention (see UI copy).
- Personal names: write Arabic names in Arabic script. For names written in Latin by the user, keep them in Latin and isolate them in RTL text (bidi.md).
- Honorifics in letters: `السيد/ السيدة`، `الأستاذ`، `الدكتور`، `سعادة` (formal Gulf).

## Marketing, WhatsApp, social posts
- Lead with the benefit in the first line, because notifications show about 40 characters.
- Use emoji sparingly, at the **end** of lines, not the start (a leading emoji can flip the line direction on some platforms).
- Every line starts with an Arabic word (bidi.md, plain-text channels). Put links on their own line.
- Ramadan/Eid greetings: `رمضان كريم`، `كل عام وأنتم بخير`، `عيد مبارك`. Avoid casual tone during Ramadan nights' campaigns for conservative markets.
- Hashtags: `#عروض_رمضان` (underscores, no spaces).
- WhatsApp formatting: `*غامق*`, `_مائل_` (renders slanted, so avoid it for Arabic), `~يتوسطه خط~`.

Example (MSA, WhatsApp):
```
عرض نهاية الأسبوع 🎉
خصم 30% على جميع العطور حتى الأحد.
اطلب الآن واستلم خلال 24 ساعة:
https://shop.example.com/ar
للاستفسار: ‎+966 50 123 4567
```
(The `‎` before `+966` is an LRM, written as `‎` in code.)

## Emails and letters
- Opening: `مرحبًا أحمد،` (friendly) · `السيد أحمد المحترم،` / `حضرة السيد…` (formal) · `السلام عليكم ورحمة الله وبركاته،` (formal, religious context).
- Closing: `مع أطيب التحيات،` · `وتفضلوا بقبول فائق الاحترام،` (formal) · `شكرًا لك،`
- Subject lines: short noun phrases: `تأكيد طلبك رقم 1024`، `تذكير بموعد الاجتماع`.
