//! Fitting a composite font's Latin to its Hangul (`style.compositeFont.fit`): how tall a font's
//! Hangul and Latin are, measured from the outlines, and the member sizes and baseline shifts that
//! give a Latin member a reference font's proportions against the Hangul. Lengths are in ‰ of the
//! em; ratios are fractions of the Hangul's height.

use crate::FontFace;
use crate::fontdb::font_outline;
use kurbo::Shape;

/// The fewest measurable Hangul syllables a font needs for its Hangul to be measured.
pub const MIN_HANGUL_SAMPLES: usize = 50;

/// Is `c` one of KS X 1001's 2,350 Hangul syllables (the ones Korean fonts draw with most care;
/// rarer syllables are often built differently and would skew a measurement)?
pub fn is_ks_hangul(c: char) -> bool {
    let Some(i) = (c as u32).checked_sub(0xAC00) else { return false };
    KS_HANGUL.get((i / 64) as usize).is_some_and(|w| (w >> (i % 64)) & 1 == 1)
}

/// KS X 1001's Hangul syllables, in code point order.
pub fn ks_hangul() -> impl Iterator<Item = char> {
    ('\u{AC00}'..='\u{D7A3}').filter(|c| is_ks_hangul(*c))
}

/// A font's Hangul: the medians of its KS X 1001 syllables' ink tops and bottoms.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HangulFace {
    pub top: f64,
    pub bottom: f64,
    pub samples: usize,
}

impl HangulFace {
    pub fn height(&self) -> f64 {
        self.top - self.bottom
    }
    pub fn center(&self) -> f64 {
        (self.top + self.bottom) / 2.0
    }
}

/// A font's Latin: the ink top and bottom of `H`, the top of `x`, and the medians of its figures'
/// tops and bottoms (when it has all ten).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LatinMetrics {
    pub cap_top: f64,
    pub cap_bottom: f64,
    pub x_height: f64,
    pub figures: Option<(f64, f64)>,
}

impl LatinMetrics {
    pub fn cap_height(&self) -> f64 {
        self.cap_top - self.cap_bottom
    }
    pub fn cap_center(&self) -> f64 {
        (self.cap_top + self.cap_bottom) / 2.0
    }
    /// Old-style figures: shorter than the capitals or hanging below the baseline. A composite
    /// font sets them like the Latin rather than fitting their height.
    pub fn oldstyle_figures(&self) -> bool {
        self.figures.is_some_and(|(bottom, top)| top - bottom < 0.85 * self.cap_height() || bottom < -50.0)
    }
}

/// The ink top and bottom of `c`'s glyph in ‰ of the em (None when unmapped or empty).
fn ink(face: &FontFace, c: char) -> Option<(f64, f64)> {
    let gid = face.glyph_for(c);
    if gid == 0 {
        return None;
    }
    let path = font_outline(face, gid);
    if path.elements().is_empty() {
        return None;
    }
    let b = path.bounding_box();
    let k = 1000.0 / face.upem.max(1.0);
    // The outline is y-down.
    let (top, bottom) = (-b.y0 * k, -b.y1 * k);
    (top.is_finite() && bottom.is_finite() && top > bottom).then_some((top, bottom))
}

/// Ink coverage of `c`'s glyph: outline area over advance × em (None when unmapped or empty).
fn coverage(face: &FontFace, c: char) -> Option<f64> {
    let gid = face.glyph_for(c);
    if gid == 0 {
        return None;
    }
    let path = font_outline(face, gid);
    let box_area = face.advance(gid) * face.upem;
    let d = path.area().abs() / box_area;
    (box_area > 0.0 && d.is_finite() && d > 0.0).then_some(d)
}

fn median(mut v: Vec<f64>) -> Option<f64> {
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n == 0 {
        return None;
    }
    let (a, b) = (v.get((n - 1) / 2)?, v.get(n / 2)?);
    Some((a + b) / 2.0)
}

fn mean(v: impl Iterator<Item = f64>) -> Option<f64> {
    let (sum, n) = v.fold((0.0, 0usize), |(s, n), x| (s + x, n + 1));
    (n > 0).then(|| sum / n as f64)
}

/// The font's Hangul, measured from its KS X 1001 syllables; None with fewer than
/// [`MIN_HANGUL_SAMPLES`] of them.
pub fn hangul_face(face: &FontFace) -> Option<HangulFace> {
    let inks: Vec<(f64, f64)> = ks_hangul().filter_map(|c| ink(face, c)).collect();
    if inks.len() < MIN_HANGUL_SAMPLES {
        return None;
    }
    let top = median(inks.iter().map(|i| i.0).collect())?;
    let bottom = median(inks.iter().map(|i| i.1).collect())?;
    (top > bottom).then_some(HangulFace { top, bottom, samples: inks.len() })
}

/// The font's Latin, measured from `H`, `x` and the figures; None without `H` or `x`.
pub fn latin_metrics(face: &FontFace) -> Option<LatinMetrics> {
    let (cap_top, cap_bottom) = ink(face, 'H')?;
    let (x_height, _) = ink(face, 'x')?;
    let digits: Vec<(f64, f64)> = ('0'..='9').filter_map(|c| ink(face, c)).collect();
    let figures =
        if digits.len() == 10 { Some((median(digits.iter().map(|d| d.1).collect())?, median(digits.iter().map(|d| d.0).collect())?)) } else { None };
    (x_height > cap_bottom).then_some(LatinMetrics { cap_top, cap_bottom, x_height, figures })
}

/// The font's Latin as its OS/2 table declares it (cap and x height over the baseline), for a
/// font without `H` or `x` to measure.
pub fn declared_latin_metrics(face: &FontFace) -> LatinMetrics {
    let k = 1000.0 / face.upem.max(1.0);
    LatinMetrics { cap_top: face.cap_height * k, cap_bottom: 0.0, x_height: face.x_height * k, figures: None }
}

/// Mean ink coverage of the font's KS X 1001 syllables, and of its lowercase Latin: Hangul lighter
/// than the Latin beside it looks bigger than it is.
pub fn densities(face: &FontFace) -> (Option<f64>, Option<f64>) {
    (mean(ks_hangul().filter_map(|c| coverage(face, c))), mean(('a'..='z').filter_map(|c| coverage(face, c))))
}

/// A font's Latin proportions against its own Hangul: cap height and x height (from the cap
/// bottom) over the Hangul's height, and the cap centre's offset from the Hangul's centre over that
/// height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ratios {
    pub cap: f64,
    pub x: f64,
    pub center: f64,
}

impl Ratios {
    pub fn of(hangul: &HangulFace, latin: &LatinMetrics) -> Option<Ratios> {
        let h = hangul.height();
        let r =
            Ratios { cap: latin.cap_height() / h, x: (latin.x_height - latin.cap_bottom) / h, center: (latin.cap_center() - hangul.center()) / h };
        (h > 0.0 && latin.cap_height() > 0.0 && [r.cap, r.x, r.center].iter().all(|v| v.is_finite())).then_some(r)
    }
}

/// Where a composite font's Hangul sits in the composite's em (‰), its entry's size and shift
/// applied.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub top: f64,
    pub bottom: f64,
}

impl Frame {
    /// `hangul` scaled by `scale` (size × vertical scale) and shifted up `shift` ‰.
    pub fn of(hangul: &HangulFace, scale: f64, shift: f64) -> Frame {
        Frame { top: hangul.top * scale + shift, bottom: hangul.bottom * scale + shift }
    }
    fn height(&self) -> f64 {
        self.top - self.bottom
    }
    fn center(&self) -> f64 {
        (self.top + self.bottom) / 2.0
    }
}

/// What a fit matches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    /// The reference's cap height and cap centre.
    Reference,
    /// The reference's x height, at the reference's cap centre.
    XHeight,
    /// The current size, at the reference's cap centre.
    PositionOnly,
}

impl Fit {
    pub const ALL: [Fit; 3] = [Fit::Reference, Fit::XHeight, Fit::PositionOnly];

    pub fn id(self) -> &'static str {
        match self {
            Fit::Reference => "reference",
            Fit::XHeight => "xHeight",
            Fit::PositionOnly => "positionOnly",
        }
    }
}

/// A member's size (1.0 = 100 %) and baseline shift (a fraction of the em, up), without Scale
/// Option, as a composite font entry holds them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub size: f64,
    pub baseline_shift: f64,
}

/// The placement giving `member` (a Latin font at vertical scale `v_scale`, now at `current`
/// size) the `target` proportions against `frame`, as `fit` asks. None when a measurement is
/// degenerate. Sizes are clamped to 1–10,000 % and shifts to ±100 % of the em.
pub fn fit(frame: &Frame, member: &LatinMetrics, v_scale: f64, current: f64, target: &Ratios, fit: Fit) -> Option<Placement> {
    let (h, cap) = (frame.height(), member.cap_height());
    if !(h > 0.0 && cap > 0.0 && v_scale > 0.0 && v_scale.is_finite()) {
        return None;
    }
    let size = match fit {
        Fit::Reference => target.cap * h / (cap * v_scale),
        Fit::XHeight => target.x * h / ((member.x_height - member.cap_bottom) * v_scale),
        Fit::PositionOnly => current,
    };
    if !(size.is_finite() && size > 0.0) {
        return None;
    }
    let size = size.clamp(0.01, 100.0);
    let shift = (frame.center() + target.center * h - size * v_scale * member.cap_center()) / 1000.0;
    shift.is_finite().then_some(Placement { size, baseline_shift: shift.clamp(-1.0, 1.0) })
}

/// The proportions `member` takes against `frame` at `size` and vertical scale `v_scale`, shifted
/// up `shift` ‰ in all.
pub fn proportions(frame: &Frame, member: &LatinMetrics, size: f64, v_scale: f64, shift: f64) -> Option<Ratios> {
    let (h, k) = (frame.height(), size * v_scale);
    let r = Ratios {
        cap: k * member.cap_height() / h,
        x: k * (member.x_height - member.cap_bottom) / h,
        center: (k * member.cap_center() + shift - frame.center()) / h,
    };
    (h > 0.0 && [r.cap, r.x, r.center].iter().all(|v| v.is_finite())).then_some(r)
}

/// KS X 1001's Hangul syllables (EUC-KR 0xB0A1–0xC8FE) as bits over U+AC00–U+D7A3: bit `i % 64` of
/// word `i / 64` is U+AC00 + `i`.
const KS_HANGUL: [u64; 175] = [
    0x1303b0113eff0793,
    0x0593000011102801,
    0x3b019703b0111e7b,
    0x306b959300a01112,
    0x113032011102b051,
    0xb879300a011102b0,
    0x0080001030011306,
    0x93000011100b0113,
    0x0593000000102b03,
    0x3b011323b051746b,
    0x7000000000001030,
    0x111029001303b011,
    0xb015300000012180,
    0x020000303001030e,
    0x1300000010230111,
    0x0113030010106b81,
    0x0000010030111013,
    0x3000000022b85530,
    0x113afb079702b011,
    0x00000021011303b0,
    0x03b011383b0d1b00,
    0x1300000111330113,
    0x00000100111c2b05,
    0x2a011300b0111000,
    0x1010000102b01930,
    0x1030030111000000,
    0x0011146b07130230,
    0x8fb8f9742b051300,
    0x00000000103b0113,
    0x01134ab0d9700000,
    0x000011030011103b,
    0x100001112ab15930,
    0x00100b0111010000,
    0x0000102b01130000,
    0x02a0111020000101,
    0x0102b05930210111,
    0x011307b019300000,
    0x00000003b011383b,
    0x383b0d1300000000,
    0x000010000103b011,
    0x0010102001130000,
    0x0000011000000100,
    0x0002181130000000,
    0x0111000000100000,
    0x0b01930000000023,
    0x302b011100301110,
    0x01303b0113c7b011,
    0xb011300000000280,
    0x03b011302b011383,
    0x1102b011300a0011,
    0x0111010000002000,
    0x2b011302a011102b,
    0x3000000101000010,
    0x11302b0113029011,
    0xb0113000000066b0,
    0x07b0113a6b07d302,
    0x1300000000200103,
    0x011303b011386b05,
    0x2b051b00000010b8,
    0x1000000003000110,
    0x79700a011102a011,
    0x0000100a0111a2b0,
    0x0090111000011100,
    0x9300000000090111,
    0x011322b0f9f2bb05,
    0x000000002001323b,
    0x303b019306b05930,
    0x117000001123a011,
    0x00001010001102b0,
    0x0000011003011301,
    0x01010010162b0793,
    0x0111020011300000,
    0x00000000b0113029,
    0x383b05130eb05130,
    0x000001000303b011,
    0x0000103901930000,
    0x000000003b000302,
    0x0000000000230113,
    0x0001000000100000,
    0x0000000290113020,
    0x1000000000000000,
    0x0000030111020000,
    0xb079b02b01130000,
    0x02b011303b011323,
    0x1343b0d9f0210111,
    0x011103b011303b01,
    0x20011322b0517020,
    0x300b011101901110,
    0x0016ab019302b011,
    0xb011302101130100,
    0x02b0313029010302,
    0x1b42b81930000000,
    0x0000033011383301,
    0x3305130000000020,
    0x0000000000001110,
    0x0130230593000001,
    0x3011101000010100,
    0x0230113000000100,
    0x1100000010100001,
    0x8513020000000000,
    0x2b01130010111003,
    0x303b011363b87730,
    0x7b30020111a2b091,
    0xf0d1702b011357f0,
    0x0ab971301b0111e3,
    0x13029001303b0113,
    0x071302b011302b01,
    0x230113033011302b,
    0x30ab011302b01130,
    0x7130090111feb411,
    0xb011307b05d347b8,
    0x0000111021015303,
    0x1102b011306b0513,
    0x0513000000103301,
    0x30000102a01038eb,
    0x3020001302b01110,
    0x001010000102b071,
    0x1011100b01130000,
    0x000000002b011300,
    0x1303b095366b0593,
    0x0000020001103b01,
    0x20000103b0113000,
    0x3000000001000010,
    0x00101001030ab011,
    0x0000000301110100,
    0x0300001023011302,
    0x0100000010000000,
    0x0000029000100000,
    0x7b01538630113000,
    0x0021015103b01130,
    0x11303b0113000000,
    0x00011010001102b0,
    0x020011102b011302,
    0x0102b01110000000,
    0x000102b011300100,
    0x2b01110000011010,
    0x002b011302101110,
    0x11302b0393000000,
    0x0000303b011302b0,
    0x03b0193000000002,
    0x0103b011102b0113,
    0x011302b011300000,
    0x0001010200001021,
    0x102b011300000010,
    0x1130200001020011,
    0x30113001011102b0,
    0x02b0113000000002,
    0x0103b011303b0313,
    0x0513000000002000,
    0x10001102b011303b,
    0x142b011300000110,
    0x0110000001000001,
    0xb011300000010280,
    0x0000001010000102,
    0x9302101110230113,
    0x0113003011100b05,
    0x3b011323b051702b,
    0x3000000000000030,
    0x11102b011303b011,
    0xb011300a01010330,
    0x0000000020000102,
    0x9300a01110000011,
    0x0000020000102b05,
    0x2901110090111000,
    0x3000000000b01110,
    0x11302b211302b011,
    0x00000020000103b0,
    0x02b011302b051300,
    0x13002011103b0113,
    0x0013028011322b21,
    0x0a011102a0113028,
    0x3021011102921130,
    0x11302b0113020011,
    0x3011122b03d30290,
    0x000000002b011302,
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FontDb;
    use crate::testing::font_mapping;

    #[test]
    fn ks_x_1001_has_its_2350_syllables() {
        assert_eq!(ks_hangul().count(), 2350);
        assert_eq!(ks_hangul().next(), Some('가'));
        assert_eq!(ks_hangul().last(), Some('힝'));
        assert!(is_ks_hangul('한') && is_ks_hangul('글'));
        // 똠 is the syllable KS X 1001 famously lacks.
        assert!(!is_ks_hangul('똠') && !is_ks_hangul('A') && !is_ks_hangul('\u{D7A4}') && !is_ks_hangul('\u{10FFFF}'));
    }

    /// A font drawing `syllables` KS syllables as Source Sans 3's `[` (its own Latin `H`, `x` and
    /// figures as themselves), and a face of it.
    fn fake_hangul(family: &str, syllables: usize, latin: bool) -> std::sync::Arc<FontFace> {
        let mut map: Vec<(char, char)> = ks_hangul().take(syllables).map(|c| (c, '[')).collect();
        map.push(('[', '['));
        if latin {
            map.extend(['H', 'x'].into_iter().chain('0'..='9').map(|c| (c, c)));
        }
        let db = FontDb::with_font_dirs(vec![]);
        db.add_font(font_mapping(family, &map).unwrap());
        db.face(family, "Regular")
    }

    #[test]
    fn hangul_and_latin_are_measured_from_the_ink() {
        let face = fake_hangul("DC Fit Measure", 2350, true);
        let bracket = ink(&face, '[').unwrap();
        let hangul = hangul_face(&face).unwrap();
        assert_eq!((hangul.top, hangul.bottom, hangul.samples), (bracket.0, bracket.1, 2350));
        let latin = latin_metrics(&face).unwrap();
        let (h, x) = (ink(&face, 'H').unwrap(), ink(&face, 'x').unwrap());
        assert_eq!((latin.cap_top, latin.cap_bottom, latin.x_height), (h.0, h.1, x.0));
        assert!(latin.cap_bottom.abs() < 1.0 && latin.cap_top > 600.0, "{latin:?}");
        assert!(latin.figures.is_some() && !latin.oldstyle_figures(), "Source Sans 3's figures are lining");
        let (hangul_density, latin_density) = densities(&face);
        assert!(hangul_density.is_some_and(|d| d > 0.0) && latin_density.is_some_and(|d| d > 0.0), "lowercase: x alone");
    }

    #[test]
    fn too_few_syllables_or_no_latin_measure_nothing() {
        assert!(hangul_face(&fake_hangul("DC Fit Few", MIN_HANGUL_SAMPLES - 1, false)).is_none());
        assert!(hangul_face(&fake_hangul("DC Fit Enough", MIN_HANGUL_SAMPLES, false)).is_some());
        assert!(latin_metrics(&fake_hangul("DC Fit No Latin", 100, false)).is_none());
    }

    #[test]
    fn oldstyle_figures_are_recognised() {
        let latin = |figures| LatinMetrics { cap_top: 700.0, cap_bottom: 0.0, x_height: 500.0, figures };
        assert!(!latin(Some((-10.0, 710.0))).oldstyle_figures());
        assert!(latin(Some((-97.0, 540.0))).oldstyle_figures(), "Georgia's");
        assert!(latin(Some((0.0, 500.0))).oldstyle_figures(), "short");
        assert!(!latin(None).oldstyle_figures());
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn each_fit_takes_on_the_reference_proportions() {
        let frame = Frame { top: 800.0, bottom: -100.0 };
        let member = LatinMetrics { cap_top: 700.0, cap_bottom: 0.0, x_height: 500.0, figures: None };
        let target = Ratios { cap: 0.75, x: 0.55, center: 0.01 };
        for v_scale in [1.0, 0.9] {
            let at = |f| {
                let p = fit(&frame, &member, v_scale, 1.2, &target, f).unwrap();
                (p, proportions(&frame, &member, p.size, v_scale, p.baseline_shift * 1000.0).unwrap())
            };
            let (p, r) = at(Fit::Reference);
            assert!(close(r.cap, target.cap) && close(r.center, target.center), "{p:?} {r:?}");
            assert!(close(p.size, 0.75 * 900.0 / (700.0 * v_scale)));
            let (_, r) = at(Fit::XHeight);
            assert!(close(r.x, target.x) && close(r.center, target.center), "{r:?}");
            let (p, r) = at(Fit::PositionOnly);
            assert!(close(p.size, 1.2) && close(r.center, target.center), "{p:?} {r:?}");
        }
    }

    #[test]
    fn a_font_fitted_to_itself_stays_as_it_is() {
        let hangul = HangulFace { top: 804.0, bottom: -67.0, samples: 2350 };
        let latin = LatinMetrics { cap_top: 713.0, cap_bottom: 42.0, x_height: 538.0, figures: None };
        let target = Ratios::of(&hangul, &latin).unwrap();
        let p = fit(&Frame::of(&hangul, 1.0, 0.0), &latin, 1.0, 1.0, &target, Fit::Reference).unwrap();
        assert!(close(p.size, 1.0) && close(p.baseline_shift, 0.0), "{p:?}");
        // A Hangul entry at 90 %, raised 20 ‰: the Latin follows it.
        let p = fit(&Frame::of(&hangul, 0.9, 20.0), &latin, 1.0, 1.0, &target, Fit::Reference).unwrap();
        assert!(close(p.size, 0.9) && close(p.baseline_shift, 0.02), "{p:?}");
    }

    #[test]
    fn degenerate_measurements_fit_nothing() {
        let member = LatinMetrics { cap_top: 700.0, cap_bottom: 0.0, x_height: 500.0, figures: None };
        let target = Ratios { cap: 0.75, x: 0.55, center: 0.0 };
        let flat = Frame { top: 100.0, bottom: 100.0 };
        assert!(fit(&flat, &member, 1.0, 1.0, &target, Fit::Reference).is_none());
        assert!(proportions(&flat, &member, 1.0, 1.0, 0.0).is_none());
        let frame = Frame { top: 800.0, bottom: -100.0 };
        for v in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(fit(&frame, &member, v, 1.0, &target, Fit::Reference).is_none(), "{v}");
        }
        let no_cap = LatinMetrics { cap_top: 0.0, ..member };
        assert!(fit(&frame, &no_cap, 1.0, 1.0, &target, Fit::Reference).is_none());
        let no_x = LatinMetrics { x_height: 0.0, ..member };
        assert!(fit(&frame, &no_x, 1.0, 1.0, &target, Fit::XHeight).is_none());
        assert!(fit(&frame, &member, 1.0, f64::NAN, &target, Fit::PositionOnly).is_none());
        // Extreme targets are clamped to what an entry can hold.
        let huge = Ratios { cap: 1e9, x: 0.5, center: 1e9 };
        let p = fit(&frame, &member, 1.0, 1.0, &huge, Fit::Reference).unwrap();
        assert_eq!((p.size, p.baseline_shift), (100.0, 1.0));
        assert!(Ratios::of(&HangulFace { top: 1.0, bottom: 1.0, samples: 50 }, &member).is_none());
    }
}
