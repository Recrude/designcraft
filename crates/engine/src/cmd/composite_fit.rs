//! Type › Composite Fonts: fitting a composite font's Latin and figures to its Hangul. The sizes
//! and baseline shifts are measured from the outlines (`designcraft_fonts::fit`) against a
//! reference font's own Latin and Hangul. The command only proposes; `style.compositeFont.set`
//! applies a candidate, so the result is plain composite font values that IDML carries.

use designcraft_doc::cjk::CompositeFontEntry;
use designcraft_fonts::fit::{self, Fit, Frame, LatinMetrics, Ratios};
use designcraft_fonts::{FontDb, ScopedFonts};
use serde_json::{Value, json};

use super::{CommandSpec, bad, cmd, has_doc, str_param};
use crate::{Result, Session};

const ID: &str = "style.compositeFont.fit";

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        query "style.compositeFont.fit",
        "Fit Composite Font",
        [],
        None,
        "{name, entry?: index, reference?: {family, style?}} → {hangul, reference, warnings, entries: [{entry, member, current, candidates: [{id: reference|xHeight|positionOnly, relativeSize, baselineShift, scaleOption, capRatio, xRatio, centerOffset}], warnings}]} — sizes and baseline shifts that give the Latin entry (and the figures' entry) the reference font's proportions against the Hangul, measured from the outlines; the reference is the Hangul member itself unless named. Proposes only: apply a candidate with style.compositeFont.set",
        has_doc,
        run
    )]
}

/// Rounded for reading: ‰ to tenths, fractions to four places (0.01 % of the em).
fn tenths(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}
fn four(v: f64) -> f64 {
    (v * 1e4).round() / 1e4
}

fn ratios_json(r: &Ratios) -> Value {
    json!({"capRatio": four(r.cap), "xRatio": four(r.x), "centerOffset": four(r.center)})
}

fn with(mut v: Value, extra: Value) -> Value {
    if let (Some(m), Value::Object(e)) = (v.as_object_mut(), extra) {
        m.extend(e);
    }
    v
}

fn run(s: &mut Session, p: &Value) -> Result<Value> {
    let name = str_param(p, "name").ok_or_else(|| bad(ID, "missing `name`"))?;
    let name = name.trim_start_matches("CompositeFont/");
    let d = &s.doc()?.doc;
    let font = d.styles.composite_fonts.iter().find(|f| f.name == name).ok_or_else(|| bad(ID, format!("no composite font `{name}`")))?;
    let db = FontDb::global().scoped(d.font_scope);
    let index = |e: &CompositeFontEntry| font.entries.iter().position(|x| std::ptr::eq(x, e));

    // The Hangul the Latin is fitted to, where its entry sets it.
    let hangul_entry = font.entry('가').ok_or_else(|| bad(ID, format!("`{name}` has no base entry")))?;
    let hangul_index = index(hangul_entry);
    if !db.has_family(&hangul_entry.family) {
        return Err(bad(ID, format!("the Hangul font `{}` is missing", hangul_entry.family)));
    }
    let hangul_face = db.face(&hangul_entry.family, &hangul_entry.style);
    let too_little = |family: &str| bad(ID, format!("`{family}` has fewer than {} KS X 1001 syllables to measure", fit::MIN_HANGUL_SAMPLES));
    let hangul = fit::hangul_face(&hangul_face).ok_or_else(|| too_little(&hangul_entry.family))?;
    let (size, _, v_scale) = hangul_entry.scales();
    let frame = Frame::of(&hangul, size * v_scale, hangul_entry.baseline_offset(1000.0, 1.0, || hangul_face.ascent_descent_center()));

    // The proportions to give the Latin: the reference font's Latin against its own Hangul.
    let (ref_family, ref_style, ref_face, ref_hangul) = match p.get("reference") {
        None | Some(Value::Null) => (hangul_entry.family.clone(), hangul_entry.style.clone(), hangul_face.clone(), hangul),
        Some(r) => {
            let family = str_param(r, "family").ok_or_else(|| bad(ID, "`reference` needs a `family`"))?;
            let style = str_param(r, "style").unwrap_or("Regular");
            if !db.has_family(family) {
                return Err(bad(ID, format!("the reference font `{family}` is missing")));
            }
            let face = db.face(family, style);
            let h = fit::hangul_face(&face).ok_or_else(|| too_little(family))?;
            (family.to_string(), style.to_string(), face, h)
        }
    };
    let ref_latin = fit::latin_metrics(&ref_face)
        .ok_or_else(|| bad(ID, format!("`{ref_family}` has no H and x to take proportions from: name a `reference` font with Hangul and Latin")))?;
    let target = Ratios::of(&ref_hangul, &ref_latin).ok_or_else(|| bad(ID, format!("`{ref_family}` can't be measured")))?;

    // The entries to fit: the one asked for, else the Latin's and the figures'.
    let mut warnings: Vec<String> = Vec::new();
    let targets: Vec<usize> = match p.get("entry") {
        None | Some(Value::Null) => {
            let mut v: Vec<usize> = Vec::new();
            for (c, what) in [('H', "the Latin is"), ('0', "the figures are")] {
                let Some((e, i)) = font.entry(c).and_then(|e| Some((e, index(e)?))) else { continue };
                if Some(i) == hangul_index || e.characters.is_empty() {
                    // The base entry keeps its size, as in InDesign.
                    let whose = if Some(i) == hangul_index { "Hangul's" } else { "base" };
                    warnings.push(format!("{what} set in the {whose} entry, which the fit leaves as it is: give it an entry of its own"));
                } else if !v.contains(&i) {
                    v.push(i);
                }
            }
            v
        }
        Some(e) => {
            let n = font.entries.len();
            let i = e
                .as_u64()
                .and_then(|i| usize::try_from(i).ok())
                .filter(|i| *i < n)
                .ok_or_else(|| bad(ID, format!("`entry` must be an entry index below {n}")))?;
            if Some(i) == hangul_index {
                return Err(bad(ID, format!("entry {i} sets the Hangul the fit measures against")));
            }
            if font.entries.get(i).is_some_and(|e| e.characters.is_empty()) {
                return Err(bad(ID, format!("entry {i} is the base entry, which keeps its size")));
            }
            vec![i]
        }
    };
    let hangul_density = fit::densities(&hangul_face).0;
    let figures_entry = font.entry('0').and_then(index);
    let entries: Vec<Value> = targets
        .into_iter()
        .filter_map(|i| font.entries.get(i).map(|e| fit_entry(&db, i, e, &frame, &target, hangul_density, figures_entry == Some(i))))
        .collect();

    Ok(json!({
        "hangul": {
            "entry": hangul_index, "family": hangul_entry.family, "style": hangul_entry.style, "samples": hangul.samples,
            "top": tenths(hangul.top), "bottom": tenths(hangul.bottom), "center": tenths(hangul.center()),
        },
        "reference": with(json!({"family": ref_family, "style": ref_style}), ratios_json(&target)),
        "warnings": warnings,
        "entries": entries,
    }))
}

/// One entry's measurements, its current proportions and the candidates.
fn fit_entry(
    db: &ScopedFonts<'_>,
    index: usize,
    e: &CompositeFontEntry,
    frame: &Frame,
    target: &Ratios,
    hangul_density: Option<f64>,
    figures: bool,
) -> Value {
    let mut warnings: Vec<String> = Vec::new();
    let member = json!({"family": e.family, "style": e.style});
    if !db.has_family(&e.family) {
        warnings.push(format!("the font `{}` is missing", e.family));
        return json!({"entry": index, "name": e.name, "member": member, "current": null, "candidates": [], "warnings": warnings});
    }
    let face = db.face(&e.family, &e.style);
    let metrics: LatinMetrics = fit::latin_metrics(&face).unwrap_or_else(|| {
        warnings.push(format!("`{}` has no H and x to measure: its declared cap and x heights are used", e.family));
        fit::declared_latin_metrics(&face)
    });
    if figures && metrics.oldstyle_figures() {
        warnings.push("old-style figures: they are fitted by the capitals, like the Latin, not by their own height".into());
    }
    if let (Some(hangul), Some(latin)) = (hangul_density, fit::densities(&face).1)
        && hangul < latin
    {
        warnings.push(format!("the Hangul is lighter than this Latin (ink {hangul:.3} against {latin:.3}): light type looks bigger, so match the weights before the size"));
    }
    let (size, _, v_scale) = e.scales();
    let shift = e.baseline_offset(1000.0, 1.0, || face.ascent_descent_center());
    let current = fit::proportions(frame, &metrics, size, v_scale, shift).map(|r| {
        with(json!({"relativeSize": four(e.relative_size), "baselineShift": four(e.baseline_shift), "scaleOption": e.scale_option}), ratios_json(&r))
    });
    let candidates: Vec<Value> = Fit::ALL
        .into_iter()
        .filter_map(|f| {
            let p = fit::fit(frame, &metrics, v_scale, size, target, f)?;
            // The proportions of the values as given (rounded), which are what gets applied.
            let (size, shift) = (four(p.size), four(p.baseline_shift));
            let r = fit::proportions(frame, &metrics, size, v_scale, shift * 1000.0)?;
            Some(with(json!({"id": f.id(), "relativeSize": size, "baselineShift": shift, "scaleOption": false}), ratios_json(&r)))
        })
        .collect();
    let mut measured = json!({
        "capTop": tenths(metrics.cap_top), "capBottom": tenths(metrics.cap_bottom), "xHeight": tenths(metrics.x_height),
    });
    if let (Some((bottom, top)), Some(m)) = (metrics.figures, measured.as_object_mut()) {
        m.insert("figures".into(), json!([tenths(bottom), tenths(top)]));
    }
    json!({
        "entry": index, "name": e.name, "member": with(member, measured),
        "current": current, "candidates": candidates, "warnings": warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use designcraft_fonts::fit::ks_hangul;
    use designcraft_fonts::testing::font_mapping;

    /// Registers (once per family) a stand-in Hangul font drawing KS X 1001's syllables as Source
    /// Sans 3's `glyph`, with its own Latin when `latin`.
    fn hangul_font(family: &str, glyph: char, latin: bool) {
        let db = FontDb::global();
        if db.has_family(family) {
            return;
        }
        let mut map: Vec<(char, char)> = ks_hangul().map(|c| (c, glyph)).collect();
        if latin {
            map.extend(['H', 'x'].into_iter().chain('0'..='9').map(|c| (c, c)));
        }
        db.add_font(font_mapping(family, &map).unwrap());
    }

    /// A Latin font whose `H` and `x` are Source Sans 3's `cap` and `x`.
    fn latin_font(family: &str, cap: char, x: char) {
        let db = FontDb::global();
        if !db.has_family(family) {
            db.add_font(font_mapping(family, &[('H', cap), ('x', x)]).unwrap());
        }
    }

    fn session_with(text: &str, entries: Value) -> (Session, designcraft_doc::StoryId) {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("frame.create", &json!({"rect": [72, 72, 500, 300], "content": "text", "text": text})).unwrap();
        let sid = s.doc().unwrap().doc.item(designcraft_doc::ItemId(r["id"].as_u64().unwrap())).unwrap().text_frame().unwrap().story;
        s.execute("style.compositeFont.set", &json!({"name": "KR", "entries": entries})).unwrap();
        s.execute("text.select", &json!({"story": sid.0, "anchor": 0, "focus": text.len()})).unwrap();
        s.execute("type.char", &json!({"fontFamily": "CompositeFont/KR"})).unwrap();
        (s, sid)
    }

    /// Ink height and centre (page points, y down) of the glyph set for story byte `byte`.
    fn ink_of(s: &Session, sid: designcraft_doc::StoryId, byte: usize) -> (f64, f64) {
        let st = s.doc().unwrap();
        let cs = s.cache.get(&st.doc, sid, None);
        let g = cs.frames.iter().flat_map(|f| &f.lines).flat_map(|l| &l.glyphs).find(|g| g.byte == byte).unwrap();
        let b = kurbo::Shape::bounding_box(&*FontDb::global().outline(g.face.get(), g.gid));
        ((b.y1 - b.y0) * g.sy, g.y + (b.y0 + b.y1) / 2.0 * g.sy)
    }

    /// Each candidate, applied, composes to the proportions it reports: what the fit measures is
    /// what composition sets.
    #[test]
    fn candidates_compose_to_the_proportions_they_report() {
        hangul_font("DC Fit Hangul", '[', true);
        latin_font("DC Fit Latin", 'O', 'o');
        let entries = json!([
            {"family": "DC Fit Hangul"},
            {"name": "$ID/Hangul", "characters": "가", "family": "DC Fit Hangul", "relativeSize": 0.95},
            {"name": "$ID/Alphabetic", "characters": "Hx", "family": "DC Fit Latin", "relativeSize": 1.2, "baselineShift": -0.05}
        ]);
        let (mut s, sid) = session_with("가H", entries.clone());
        let r = s.execute(ID, &json!({"name": "KR"})).unwrap();
        assert_eq!(r["hangul"]["entry"], 1, "{r}");
        assert_eq!(r["hangul"]["samples"], 2350);
        assert_eq!(r["reference"]["family"], "DC Fit Hangul");
        let entry = &r["entries"][0];
        assert_eq!(entry["entry"], 2, "{r}");
        let ids: Vec<&str> = entry["candidates"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["reference", "xHeight", "positionOnly"]);
        assert_eq!(entry["current"]["relativeSize"], 1.2);
        for c in entry["candidates"].as_array().unwrap() {
            let mut e = entries.clone();
            e[2]["relativeSize"] = c["relativeSize"].clone();
            e[2]["baselineShift"] = c["baselineShift"].clone();
            e[2]["scaleOption"] = c["scaleOption"].clone();
            s.execute("style.compositeFont.set", &json!({"name": "KR", "entries": e})).unwrap();
            let ((hh, hc), (lh, lc)) = (ink_of(&s, sid, 0), ink_of(&s, sid, 3));
            let (cap, center) = (lh / hh, (hc - lc) / hh);
            let id = &c["id"];
            assert!((cap - c["capRatio"].as_f64().unwrap()).abs() < 1e-3, "{id}: cap {cap} vs {c}");
            assert!((center - c["centerOffset"].as_f64().unwrap()).abs() < 1e-3, "{id}: centre {center} vs {c}");
        }
        // The reference candidate takes on the reference's proportions.
        let reference = &entry["candidates"][0];
        for k in ["capRatio", "centerOffset"] {
            assert!((reference[k].as_f64().unwrap() - r["reference"][k].as_f64().unwrap()).abs() < 1e-3, "{k}: {r}");
        }
    }

    /// The Latin and the figures each get an entry of their own fitted; old-style figures and a
    /// missing member are warnings, not failures.
    #[test]
    fn the_latin_and_figures_entries_are_fitted_with_warnings() {
        hangul_font("DC Fit Hangul", '[', true);
        latin_font("DC Fit Latin", 'O', 'o');
        let entries = json!([
            {"family": "DC Fit Hangul"},
            {"characters": "ABHx", "family": "DC Fit Latin"},
            {"characters": "0123456789", "family": "DC Fit Missing Figures"}
        ]);
        let (mut s, _) = session_with("가H0", entries);
        let r = s.execute(ID, &json!({"name": "CompositeFont/KR"})).unwrap();
        assert_eq!(r["hangul"]["entry"], 0, "the base sets the Hangul here: {r}");
        assert!(r["warnings"].as_array().unwrap().is_empty(), "{r}");
        let entries = r["entries"].as_array().unwrap();
        assert_eq!(entries.iter().map(|e| e["entry"].as_u64().unwrap()).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(entries[0]["candidates"].as_array().unwrap().len(), 3);
        assert!(entries[1]["candidates"].as_array().unwrap().is_empty() && entries[1]["current"].is_null());
        assert!(entries[1]["warnings"][0].as_str().unwrap().contains("missing"), "{r}");
        // Only the Hangul's entry: nothing to fit, said so.
        let r = s.execute("style.compositeFont.set", &json!({"name": "Solo", "entries": [{"family": "DC Fit Hangul"}]})).unwrap();
        assert_eq!(r["name"], "Solo");
        let r = s.execute(ID, &json!({"name": "Solo"})).unwrap();
        assert!(r["entries"].as_array().unwrap().is_empty() && !r["warnings"].as_array().unwrap().is_empty(), "{r}");
    }

    #[test]
    fn a_reference_font_sets_the_proportions() {
        hangul_font("DC Fit Hangul", '[', true);
        hangul_font("DC Fit Hangul Only", '[', false);
        // `l` is shorter than `[`: this reference's Hangul is short against its Latin.
        hangul_font("DC Fit Short Reference", 'l', true);
        latin_font("DC Fit Latin", 'O', 'o');
        let entries = json!([{"family": "DC Fit Hangul Only"}, {"characters": "Hx", "family": "DC Fit Latin"}]);
        let (mut s, _) = session_with("가H", entries);
        // The Hangul font has no Latin: a reference is needed.
        let e = s.execute(ID, &json!({"name": "KR"})).unwrap_err().to_string();
        assert!(e.contains("reference"), "{e}");
        let own = s.execute(ID, &json!({"name": "KR", "reference": {"family": "DC Fit Hangul"}})).unwrap();
        let short = s.execute(ID, &json!({"name": "KR", "reference": {"family": "DC Fit Short Reference"}})).unwrap();
        assert_eq!(own["reference"]["family"], "DC Fit Hangul");
        let size = |r: &Value| r["entries"][0]["candidates"][0]["relativeSize"].as_f64().unwrap();
        assert!(size(&short) > size(&own), "a reference with shorter Hangul makes the Latin bigger: {own} {short}");
    }

    #[test]
    fn bad_requests_are_errors() {
        hangul_font("DC Fit Hangul", '[', true);
        let entries = json!([{"family": "DC Fit Hangul"}, {"characters": "Hx", "family": "DC Fit Hangul"}]);
        let (mut s, _) = session_with("가H", entries);
        for p in [
            json!({}),
            json!({"name": 3}),
            json!({"name": "Nope"}),
            json!({"name": "KR", "entry": 9}),
            json!({"name": "KR", "entry": -1}),
            json!({"name": "KR", "entry": "1"}),
            json!({"name": "KR", "entry": 0}),
            json!({"name": "KR", "reference": {}}),
            json!({"name": "KR", "reference": "DC Fit Hangul"}),
            json!({"name": "KR", "reference": {"family": "No Such Font"}}),
            json!([1, 2]),
        ] {
            assert!(s.execute(ID, &p).is_err(), "{p}");
        }
        // A Hangul font too small to measure.
        s.execute(
            "style.compositeFont.set",
            &json!({"name": "Missing", "entries": [{"family": "No Such Hangul"}, {"characters": "H", "family": "DC Fit Hangul"}]}),
        )
        .unwrap();
        assert!(s.execute(ID, &json!({"name": "Missing"})).is_err());
    }
}
