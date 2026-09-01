// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The device model catalog.
//!
//! Every block on the grid is stored as an integer model id. The catalog is
//! what turns that into a name, a category, and the parameter list in wire
//! index order. It comes FROM the device, so it covers installed block types,
//! including purchased plugin models. Its Neural Capture entries are capture
//! block types, not the unit's inventory of individual captures; that inventory
//! is reported separately through `File` listings.
//!
//! ## Container
//!
//! Confirmed against `CorOS` 4.0.1 (firmware `d14e`) on 2026-08-02: the
//! `ModelRepo` payload is `gzip(tar(ModelRepo.xml))`. On that unit it was
//! 46,704 bytes gzipped, 558,592 bytes of tar, and a 556,732-byte XML
//! document describing 533 models in 31 categories with 3,809 parameters.
//!
//! Note this gzip is the FIELD-level one (inside a protobuf `bytes` field),
//! distinct from the frame-level gzip the transport already unwraps.
//!
//! ## Attribution
//!
//! Each `Model` may carry a `tm` attribute holding **Neural DSP's own**
//! trademark attribution, e.g. `Based on Marshall® JCM800®`. This crate
//! surfaces that string verbatim as [`Model::based_on`] and never
//! paraphrases it: it is their wording about other companies' marks, and
//! reproducing it exactly is both more accurate and safer than inventing a
//! mapping. See the note on [`Model::based_on`].
//!
//! The catalog is read from the device at runtime. Its contents are Neural
//! DSP's, and must not be committed into this repository.
//!
//! @see spec/130-domain-model/spec.md
//! @see spec/150-client/spec.md [FR-7]

use std::collections::HashMap;

/// What kind of control a parameter is, as declared by the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ParameterKind {
    /// A continuous value over `min..max`.
    Float,
    /// An integer value over `min..max`.
    Int,
    /// A discrete selector; see [`Parameter::step_names`].
    Switch,
    /// A string value, e.g. a cabinet's microphone selection or a capture's
    /// library key.
    Str,
    /// A level fader.
    Fader,
    /// A live READ-ONLY meter, not a setting. Writing to one is meaningless.
    Meter,
    /// A declared-but-unused slot. Present so positional indices stay aligned.
    Empty,
    /// A type this crate does not recognise. Carried through rather than
    /// dropped, so an unknown does not silently shift later indices.
    Unknown,
}

impl ParameterKind {
    fn parse(s: &str) -> Self {
        match s {
            "float" => Self::Float,
            "int" => Self::Int,
            "switch" => Self::Switch,
            "string" => Self::Str,
            "fader" => Self::Fader,
            "meter" => Self::Meter,
            "empty" => Self::Empty,
            _ => Self::Unknown,
        }
    }

    /// Whether this parameter is a readable measurement rather than a
    /// writable setting.
    #[must_use]
    pub const fn is_read_only(self) -> bool {
        matches!(self, Self::Meter)
    }
}

/// One parameter of a model, in wire index order.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Parameter {
    /// The WIRE index: this parameter's position within its model.
    ///
    /// This is what `set_param` addresses, and it is positional - derived
    /// from the order parameters appear in the catalog, not from any id
    /// attribute. `Empty` and `Meter` entries still occupy an index, which is
    /// why they are retained rather than filtered out.
    pub index: usize,
    /// Display name as the unit shows it, e.g. `GAIN`.
    pub name: String,
    /// The kind of control.
    pub kind: ParameterKind,
    /// Minimum value in the parameter's own units.
    pub min: f64,
    /// Maximum value in the parameter's own units.
    pub max: f64,
    /// Default value in the parameter's own units.
    pub default: f64,
    /// Units string, often empty.
    pub units: String,
    /// For a switch, the option labels in order.
    pub step_names: Vec<String>,
}

impl Parameter {
    /// Convert a value in this parameter's own units to the normalised 0..1
    /// float the wire carries.
    ///
    /// Returns `None` when the declared range is degenerate (`max == min`),
    /// which some catalog entries are - the range is a placeholder for those
    /// and no meaningful conversion exists.
    #[must_use]
    pub fn to_normalised(&self, real: f64) -> Option<f64> {
        let span = self.max - self.min;
        if span == 0.0 || !span.is_finite() {
            return None;
        }
        Some(((real - self.min) / span).clamp(0.0, 1.0))
    }

    /// Convert a normalised 0..1 wire value back to the parameter's own units.
    #[must_use]
    pub fn from_normalised(&self, normalised: f64) -> Option<f64> {
        let span = self.max - self.min;
        if span == 0.0 || !span.is_finite() {
            return None;
        }
        Some(self.min + normalised * span)
    }
}

/// One model: an amp, pedal, cab, capture, or utility block.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Model {
    /// The integer id a preset stores to reference this model.
    pub id: u32,
    /// Display name, e.g. `Brit 2203`.
    pub name: String,
    /// The id of the category this model belongs to.
    pub category_id: u32,
    /// The category's display name, e.g. `Guitar Amplifier`.
    pub category: String,
    /// **Neural DSP's own** attribution for what this model is based on,
    /// e.g. `Based on Marshall® JCM800®`. Absent for models that are not
    /// modelled on identifiable third-party gear (utilities, captures, and
    /// most cabs).
    ///
    /// Reproduce this string VERBATIM. It concerns other companies'
    /// trademarks, it is Neural DSP's carefully-worded statement about them,
    /// and paraphrasing it - or presenting our own mapping as authoritative -
    /// would be both less accurate and less defensible. On the unit measured,
    /// 307 of 533 models carried one.
    pub based_on: Option<String>,
    /// Parameters in wire index order.
    pub parameters: Vec<Parameter>,
}

impl Model {
    /// Find a parameter by name, case-insensitively.
    ///
    /// Naming is the safer way to address a parameter: indices are positional
    /// and not every one is a visible knob, so writing a guessed index can
    /// change stored data while moving nothing on screen.
    #[must_use]
    pub fn parameter(&self, name: &str) -> Option<&Parameter> {
        let wanted = name.trim().to_lowercase();
        self.parameters
            .iter()
            .find(|p| p.name.trim().to_lowercase() == wanted)
    }
}

/// The device's model catalog, keyed by model id.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Catalog {
    models: HashMap<u32, Model>,
}

impl Catalog {
    /// Parse a catalog from the raw `ModelRepo` payload.
    ///
    /// Accepts the payload exactly as [`crate::QuadCortex::fetch_model_repo`]
    /// returns it: `gzip(tar(ModelRepo.xml))`. Gunzipping and tar extraction
    /// are handled here.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::Decode`] if the payload is not gzip, does not
    /// contain a tar holding `ModelRepo.xml`, or the XML cannot be parsed.
    pub fn parse(payload: &[u8]) -> crate::Result<Self> {
        let xml = extract_model_repo_xml(payload)?;
        Self::from_xml(&xml)
    }

    /// Parse a catalog from the already-extracted `ModelRepo.xml` text.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::Decode`] on malformed XML.
    pub fn from_xml(xml: &str) -> crate::Result<Self> {
        use quick_xml::events::Event;

        let mut reader = quick_xml::Reader::from_str(xml);
        reader.config_mut().trim_text(true);

        let mut models: HashMap<u32, Model> = HashMap::new();
        let mut category_id: u32 = 0;
        let mut category_name = String::new();
        let mut current: Option<Model> = None;

        loop {
            let event = reader
                .read_event()
                .map_err(|e| crate::Error::Decode(format!("ModelRepo.xml: {e}")))?;

            match event {
                Event::Eof => break,
                Event::Start(ref e) | Event::Empty(ref e) => {
                    let is_empty = matches!(event, Event::Empty(_));
                    match e.name().as_ref() {
                        "Category" => {
                            let attrs = attributes(e)?;
                            category_id = attrs
                                .get("id")
                                .and_then(|v| v.parse().ok())
                                .unwrap_or_default();
                            category_name = attrs.get("name").cloned().unwrap_or_default();
                        }
                        "Model" => {
                            let attrs = attributes(e)?;
                            let Some(id) = attrs.get("id").and_then(|v| v.parse::<u32>().ok())
                            else {
                                continue;
                            };
                            let model = Model {
                                id,
                                name: attrs.get("name").cloned().unwrap_or_default(),
                                category_id,
                                category: category_name.clone(),
                                // Empty string means "no attribution", not
                                // "attributed to nothing".
                                based_on: attrs.get("tm").filter(|v| !v.trim().is_empty()).cloned(),
                                parameters: Vec::new(),
                            };
                            if is_empty {
                                models.insert(id, model);
                            } else {
                                // A Model with children; finish it on its end tag.
                                if let Some(prev) = current.replace(model) {
                                    models.insert(prev.id, prev);
                                }
                            }
                        }
                        "Parameter" => {
                            if let Some(model) = current.as_mut() {
                                let attrs = attributes(e)?;
                                let steps = attrs
                                    .get("stepNames")
                                    .filter(|v| !v.is_empty())
                                    .map(|v| v.split(',').map(|s| s.trim().to_string()).collect())
                                    .unwrap_or_default();
                                model.parameters.push(Parameter {
                                    // Positional: this IS the wire index.
                                    index: model.parameters.len(),
                                    name: attrs.get("name").cloned().unwrap_or_default(),
                                    kind: ParameterKind::parse(
                                        attrs.get("type").map_or("", String::as_str),
                                    ),
                                    min: attrs
                                        .get("min")
                                        .and_then(|v| v.parse().ok())
                                        .unwrap_or(0.0),
                                    max: attrs
                                        .get("max")
                                        .and_then(|v| v.parse().ok())
                                        .unwrap_or(0.0),
                                    default: attrs
                                        .get("defaultValue")
                                        .and_then(|v| v.parse().ok())
                                        .unwrap_or(0.0),
                                    units: attrs.get("units").cloned().unwrap_or_default(),
                                    step_names: steps,
                                });
                            }
                        }
                        _ => {}
                    }
                }
                Event::End(ref e) if e.name().as_ref() == "Model" => {
                    if let Some(model) = current.take() {
                        models.insert(model.id, model);
                    }
                }
                _ => {}
            }
        }

        if let Some(model) = current.take() {
            models.insert(model.id, model);
        }

        if models.is_empty() {
            return Err(crate::Error::Decode(
                "ModelRepo.xml contained no models".into(),
            ));
        }
        Ok(Self { models })
    }

    /// Look a model up by the id a preset stores.
    #[must_use]
    pub fn get(&self, id: u32) -> Option<&Model> {
        self.models.get(&id)
    }

    /// How many models the catalog holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.models.len()
    }

    /// Whether the catalog is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.models.is_empty()
    }

    /// Every model, in ascending id order.
    #[must_use]
    pub fn models(&self) -> Vec<&Model> {
        let mut all: Vec<&Model> = self.models.values().collect();
        all.sort_by_key(|m| m.id);
        all
    }

    /// Find models whose name contains `needle`, case-insensitively.
    #[must_use]
    pub fn search(&self, needle: &str) -> Vec<&Model> {
        let wanted = needle.trim().to_lowercase();
        let mut found: Vec<&Model> = self
            .models
            .values()
            .filter(|m| {
                m.name.to_lowercase().contains(&wanted)
                    || m.based_on
                        .as_ref()
                        .is_some_and(|t| t.to_lowercase().contains(&wanted))
            })
            .collect();
        found.sort_by_key(|m| m.id);
        found
    }
}

/// Gunzip the payload and pull `ModelRepo.xml` out of the tar inside it.
///
/// The tar is hand-parsed rather than pulled in as a dependency: it holds a
/// single file, and the ustar header is a fixed, well-documented layout
/// (name at 0, size as octal at 124, content at 512, blocks of 512).
fn extract_model_repo_xml(payload: &[u8]) -> crate::Result<String> {
    use std::io::Read;

    if payload.len() < 2 || payload[0] != 0x1f || payload[1] != 0x8b {
        return Err(crate::Error::Decode(format!(
            "ModelRepo payload is not gzip (leading bytes {:02x?})",
            &payload[..payload.len().min(4)]
        )));
    }

    let mut tar = Vec::new();
    flate2::read::GzDecoder::new(payload)
        .read_to_end(&mut tar)
        .map_err(|e| crate::Error::Decode(format!("ModelRepo gunzip failed: {e}")))?;

    // Walk tar headers until we find ModelRepo.xml.
    let mut offset = 0usize;
    while offset + 512 <= tar.len() {
        let header = &tar[offset..offset + 512];
        // Two consecutive zero blocks mark end-of-archive; one is enough to stop.
        if header.iter().all(|&b| b == 0) {
            break;
        }
        let name = cstr(&header[0..100]);
        let size = octal(&header[124..136]).ok_or_else(|| {
            crate::Error::Decode(format!("tar entry {name:?} has an unreadable size field"))
        })?;
        let start = offset + 512;
        let end = start
            .checked_add(size)
            .filter(|e| *e <= tar.len())
            .ok_or_else(|| {
                crate::Error::Decode(format!(
                    "tar entry {name:?} runs past the end of the archive"
                ))
            })?;

        if name.ends_with("ModelRepo.xml") {
            return String::from_utf8(tar[start..end].to_vec())
                .map_err(|e| crate::Error::Decode(format!("ModelRepo.xml is not UTF-8: {e}")));
        }

        // Entries are padded to a 512-byte boundary.
        offset = start + size.div_ceil(512) * 512;
    }

    Err(crate::Error::Decode(
        "ModelRepo tar contained no ModelRepo.xml".into(),
    ))
}

/// Read a NUL-terminated field from a tar header.
fn cstr(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).trim().to_string()
}

/// Parse a tar header's octal size field.
fn octal(bytes: &[u8]) -> Option<usize> {
    let text = cstr(bytes);
    let digits = text.trim();
    if digits.is_empty() {
        return Some(0);
    }
    usize::from_str_radix(digits, 8).ok()
}

/// Collect an element's attributes into a map.
fn attributes(e: &quick_xml::events::BytesStart<'_>) -> crate::Result<HashMap<String, String>> {
    e.attributes()
        .map(|attribute| {
            let attribute = attribute
                .map_err(|e| crate::Error::Decode(format!("ModelRepo.xml attribute: {e}")))?;
            let key = attribute.key.as_ref().to_owned();
            // XML 1.0 attribute-value normalisation. The catalog declares
            // `<?xml version="1.0" ?>`, so 1.0 is the correct rule set.
            let value = attribute
                .normalized_value(quick_xml::XmlVersion::Explicit1_0)
                .map_err(|e| crate::Error::Decode(format!("ModelRepo.xml attribute: {e}")))?
                .to_string();
            Ok((key, value))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A miniature ModelRepo.xml in the shape the device sends, including the
    /// `tm` attribution and a switch with step names.
    const SAMPLE: &str = r#"<?xml version="1.0" ?><Models><Category id="0" name="Guitar Overdrive"><Model blob="x" id="1" name="Myth Drive" style="" tm="Based on Klon(R) Centaur(R)"><Padding cpu="1.0"/><Parameter defaultValue="5" max="10" min="0" name="GAIN" type="float" units=""/><Parameter defaultValue="0" max="1" min="0" name="PEAK" stepNames="LP,HP" steps="2" type="switch" units=""/></Model></Category><Category id="1" name="Guitar Amplifier"><Model blob="y" id="1001" name="Brit 2203" style="" tm="Based on Marshall(R) JCM800(R)"><Parameter defaultValue="5" max="10" min="0" name="GAIN" type="float" units=""/><Parameter defaultValue="0" max="1" min="0" name="INPUT" type="meter" units=""/></Model><Model blob="z" id="2000" name="My Capture" style="" tm=""/></Category></Models>"#;

    fn sample() -> Catalog {
        Catalog::from_xml(SAMPLE).expect("sample parses")
    }

    fn model_repo_payload(xml: &[u8]) -> Vec<u8> {
        use std::io::Write as _;

        let mut tar = vec![0; 512];
        tar[.."ModelRepo.xml".len()].copy_from_slice(b"ModelRepo.xml");
        let size = format!("{:011o}\0", xml.len());
        tar[124..136].copy_from_slice(size.as_bytes());
        tar.extend_from_slice(xml);
        tar.resize(512 + xml.len().div_ceil(512) * 512 + 1024, 0);

        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gzip.write_all(&tar).expect("fixture compresses");
        gzip.finish().expect("fixture finishes")
    }

    #[test]
    fn parses_models_and_categories() {
        let c = sample();
        assert_eq!(c.len(), 3);
        let drive = c.get(1).unwrap();
        assert_eq!(drive.name, "Myth Drive");
        assert_eq!(drive.category, "Guitar Overdrive");
        assert_eq!(drive.category_id, 0);
        let amp = c.get(1001).unwrap();
        assert_eq!(amp.category, "Guitar Amplifier");
    }

    #[test]
    fn carries_vendor_attribution_verbatim() {
        let c = sample();
        assert_eq!(
            c.get(1001).unwrap().based_on.as_deref(),
            Some("Based on Marshall(R) JCM800(R)")
        );
    }

    #[test]
    fn decodes_escaped_attribute_values() {
        let catalog = Catalog::from_xml(
            r#"<Models><Category id="1" name="A &amp; B"><Model id="1" name="M &amp; M"/></Category></Models>"#,
        )
        .unwrap();

        assert_eq!(catalog.get(1).unwrap().name, "M & M");
        assert_eq!(catalog.get(1).unwrap().category, "A & B");
    }

    #[test]
    fn empty_attribution_is_absent_not_blank() {
        // A capture has tm="" - that means "no attribution", and a caller
        // checking `is_some()` should not be told there is one.
        let c = sample();
        assert_eq!(c.get(2000).unwrap().based_on, None);
    }

    #[test]
    fn parameter_index_is_positional() {
        // The wire index is the position, not any id attribute. Getting this
        // wrong writes to the wrong knob while reading back cleanly.
        let c = sample();
        let drive = c.get(1).unwrap();
        assert_eq!(drive.parameters[0].name, "GAIN");
        assert_eq!(drive.parameters[0].index, 0);
        assert_eq!(drive.parameters[1].name, "PEAK");
        assert_eq!(drive.parameters[1].index, 1);
    }

    #[test]
    fn switch_step_names_are_split() {
        let c = sample();
        let peak = c.get(1).unwrap().parameter("peak").unwrap();
        assert_eq!(peak.kind, ParameterKind::Switch);
        assert_eq!(peak.step_names, vec!["LP", "HP"]);
    }

    #[test]
    fn parameter_lookup_is_case_insensitive() {
        let c = sample();
        assert!(c.get(1).unwrap().parameter("gain").is_some());
        assert!(c.get(1).unwrap().parameter("  GAIN ").is_some());
        assert!(c.get(1).unwrap().parameter("nope").is_none());
    }

    #[test]
    fn meters_are_flagged_read_only() {
        // A meter occupies a wire index but is a measurement, not a setting.
        let c = sample();
        let input = c.get(1001).unwrap().parameter("INPUT").unwrap();
        assert!(input.kind.is_read_only());
        assert_eq!(input.index, 1);
    }

    #[test]
    fn normalisation_round_trips() {
        let c = sample();
        let gain = c.get(1).unwrap().parameter("GAIN").unwrap();
        assert_eq!(gain.to_normalised(5.0), Some(0.5));
        assert_eq!(gain.from_normalised(0.5), Some(5.0));
        // Out of range clamps rather than producing a value the device rejects.
        assert_eq!(gain.to_normalised(99.0), Some(1.0));
    }

    #[test]
    fn degenerate_range_has_no_conversion() {
        // Some catalog ranges are placeholders (min == max). Converting
        // against one would divide by zero; report it instead.
        let p = Parameter {
            index: 0,
            name: "X".into(),
            kind: ParameterKind::Float,
            min: 0.0,
            max: 0.0,
            default: 0.0,
            units: String::new(),
            step_names: Vec::new(),
        };
        assert_eq!(p.to_normalised(1.0), None);
        assert_eq!(p.from_normalised(0.5), None);
    }

    #[test]
    fn search_matches_name_and_attribution() {
        let c = sample();
        assert_eq!(c.search("brit").len(), 1);
        // Searching by the real-world gear it evokes should work too.
        assert_eq!(c.search("marshall").len(), 1);
        assert_eq!(c.search("nothing here").len(), 0);
    }

    #[test]
    fn rejects_a_non_gzip_payload() {
        let err = Catalog::parse(b"not gzip at all").unwrap_err();
        assert!(matches!(err, crate::Error::Decode(_)));
    }

    #[test]
    fn rejects_non_utf8_model_repo_xml() {
        let payload = model_repo_payload(b"<Models><Model id=\"1\" name=\"\xff\"/></Models>");
        let err = Catalog::parse(&payload).unwrap_err();
        assert!(matches!(err, crate::Error::Decode(_)));
    }

    #[test]
    fn rejects_duplicate_attributes() {
        let err = Catalog::from_xml(
            r#"<Models><Category id="1" name="A"><Model id="1" id="2" name="M"/></Category></Models>"#,
        )
        .unwrap_err();
        assert!(matches!(err, crate::Error::Decode(_)));
    }

    #[test]
    fn rejects_xml_with_no_models() {
        let err = Catalog::from_xml("<?xml version=\"1.0\" ?><Models></Models>").unwrap_err();
        assert!(matches!(err, crate::Error::Decode(_)));
    }
}
