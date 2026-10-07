//! A bounded event walk over XML with the policies every evidence parser needs.

use std::collections::BTreeMap;

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::{EvidenceError, MAX_DEPTH, MAX_ELEMENTS, check_size};

pub(crate) enum Node {
    Open { name: String, attrs: BTreeMap<String, String> },
    Close { name: String },
}

fn attributes(start: &BytesStart<'_>, format: &'static str) -> Result<BTreeMap<String, String>, EvidenceError> {
    let mut out = BTreeMap::new();
    for attr in start.attributes() {
        let attr = attr.map_err(|e| EvidenceError::Malformed { format, message: e.to_string() })?;
        let key = attr.key.local_name().as_ref().to_owned();
        // Only the five predefined entities and character references are decoded; anything else
        // is an error, so a document cannot smuggle in entity expansion.
        let value = attr
            .normalized_value(quick_xml::XmlVersion::Explicit1_0)
            .map_err(|e| EvidenceError::Malformed { format, message: e.to_string() })?;
        out.insert(key, value.into_owned());
    }
    Ok(out)
}

/// Calls `visit` for each element start (empty elements produce Open then Close), end and text.
pub(crate) fn walk(
    input: &str,
    format: &'static str,
    mut visit: impl FnMut(Node) -> Result<(), EvidenceError>,
) -> Result<(), EvidenceError> {
    check_size(input)?;
    let mut reader = Reader::from_str(input);
    reader.config_mut().trim_text(true);
    let mut depth = 0usize;
    let mut elements = 0usize;
    loop {
        let event = reader.read_event().map_err(|e| EvidenceError::Malformed { format, message: e.to_string() })?;
        match event {
            Event::DocType(doctype) => {
                // JaCoCo emits a DOCTYPE naming an external DTD; that is harmless because nothing
                // is ever fetched. An internal subset declaring entities is not.
                if AsRef::<str>::as_ref(&doctype).contains("<!ENTITY") {
                    return Err(EvidenceError::EntityDeclaration { format });
                }
            }
            Event::Start(start) => {
                depth += 1;
                elements += 1;
                if depth > MAX_DEPTH {
                    return Err(EvidenceError::LimitExceeded { format, what: "nesting depth" });
                }
                if elements > MAX_ELEMENTS {
                    return Err(EvidenceError::LimitExceeded { format, what: "element count" });
                }
                let name = start.local_name().as_ref().to_owned();
                visit(Node::Open { name, attrs: attributes(&start, format)? })?;
            }
            Event::Empty(start) => {
                elements += 1;
                if elements > MAX_ELEMENTS {
                    return Err(EvidenceError::LimitExceeded { format, what: "element count" });
                }
                let name = start.local_name().as_ref().to_owned();
                visit(Node::Open { name: name.clone(), attrs: attributes(&start, format)? })?;
                visit(Node::Close { name })?;
            }
            Event::End(end) => {
                depth = depth.saturating_sub(1);
                visit(Node::Close { name: end.local_name().as_ref().to_owned() })?;
            }
            // Element text (stack traces, stdout) is not used by any evidence format here.
            Event::Text(_) | Event::CData(_) => {}
            Event::Eof => return Ok(()),
            _ => {}
        }
    }
}
