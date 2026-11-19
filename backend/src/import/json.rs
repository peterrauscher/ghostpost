//! Bounded JSON reads shared by archive parsers.
//!
//! Archive entries are already capped by the ZIP index, but a parser may also
//! be handed an arbitrary reader in tests or by a future caller. Every read
//! here enforces the same limits itself: the selected-entry byte cap while
//! reading, `MAX_JSON_NESTING`, and `MAX_JSON_ELEMENT_BYTES` for one record.
//! Collections are streamed one top-level element at a time.
use super::{limits, ImportError, ImportErrorCode};
use serde_json::Value;
use std::io::{self, BufReader, Read, Write};

const BOM: [u8; 3] = [0xef, 0xbb, 0xbf];

fn integrity() -> ImportError {
    ImportErrorCode::IntegrityMismatch.into()
}

fn byte<R: Read>(reader: &mut R) -> Result<Option<u8>, ImportError> {
    let mut one = [0u8; 1];
    loop {
        match reader.read(&mut one) {
            Ok(0) => return Ok(None),
            Ok(_) => return Ok(Some(one[0])),
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(integrity()),
        }
    }
}

fn is_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\n' | b'\r' | b'\t')
}

fn trim_ascii(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(|byte| is_whitespace(*byte)) {
        bytes = &bytes[1..];
    }
    while bytes.last().is_some_and(|byte| is_whitespace(*byte)) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

fn check_depth(value: &Value) -> Result<(), ImportError> {
    fn walk(value: &Value, depth: usize) -> Result<(), ImportError> {
        match value {
            Value::Array(values) => {
                if depth > limits::MAX_JSON_NESTING {
                    return Err(ImportErrorCode::ResourceLimit.into());
                }
                for value in values {
                    walk(value, depth + 1)?;
                }
            }
            Value::Object(values) => {
                if depth > limits::MAX_JSON_NESTING {
                    return Err(ImportErrorCode::ResourceLimit.into());
                }
                for value in values.values() {
                    walk(value, depth + 1)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    walk(value, 1)
}

struct Counted {
    written: usize,
}

impl Write for Counted {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.written = self.written.saturating_add(buffer.len());
        if self.written > limits::MAX_JSON_ELEMENT_BYTES {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "element too large"));
        }
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Validates one already-parsed record: nesting must not exceed
/// `MAX_JSON_NESTING`, and its serialized form must not exceed
/// `MAX_JSON_ELEMENT_BYTES`.
pub fn check_element(value: &Value) -> Result<(), ImportError> {
    check_depth(value)?;
    let mut counter = Counted { written: 0 };
    serde_json::to_writer(&mut counter, value)
        .map_err(|_| ImportError::from(ImportErrorCode::ResourceLimit))?;
    Ok(())
}

/// Parses one raw JSON value obtained from `stream_array`, applying
/// `check_element` after decoding.
pub fn parse_element(bytes: &[u8]) -> Result<Value, ImportError> {
    if bytes.len() > limits::MAX_JSON_ELEMENT_BYTES { return Err(ImportErrorCode::ResourceLimit.into()); }
    std::str::from_utf8(bytes).map_err(|_| ImportError::from(ImportErrorCode::InvalidEncoding))?;
    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| ImportError::from(ImportErrorCode::InvalidSchema))?;
    check_element(&value)?;
    Ok(value)
}

fn wrapper_key(prefix: &[u8]) -> Result<Option<String>, ImportError> {
    let prefix = prefix.strip_prefix(BOM.as_slice()).unwrap_or(prefix);
    let trimmed = trim_ascii(prefix);
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.first() != Some(&b'{') {
        return Err(ImportErrorCode::InvalidSchema.into());
    }
    // The prefix ends just before the array's `[`; completing the object with a
    // null value makes the wrapper (and any earlier keys) parseable as JSON.
    let mut document = trimmed.to_vec();
    document.extend_from_slice(b"null}");
    let value: Value = serde_json::from_slice(&document)
        .map_err(|_| ImportError::from(ImportErrorCode::InvalidSchema))?;
    let object = value
        .as_object()
        .ok_or_else(|| ImportError::from(ImportErrorCode::InvalidSchema))?;
    if object.len() != 1 {
        return Err(ImportErrorCode::UnsupportedFormatVersion.into());
    }
    let (key, value) = object.iter().next().expect("one key");
    if !value.is_null() {
        return Err(ImportErrorCode::UnsupportedFormatVersion.into());
    }
    Ok(Some(key.clone()))
}

fn limited_byte<R: Read>(reader: &mut R, consumed: &mut u64) -> Result<Option<u8>, ImportError> {
    let byte = byte(reader)?;
    if byte.is_some() {
        *consumed = consumed.saturating_add(1);
        if *consumed > limits::MAX_SELECTED_ENTRY_UNCOMPRESSED_BYTES {
            return Err(ImportErrorCode::ResourceLimit.into());
        }
    }
    Ok(byte)
}

/// Streams the elements of a top-level JSON array, or of the array under the
/// single key of a top-level object (`{"ig_stories": [ ... ]}`). The callback
/// receives the wrapper key (`None` for a bare array) and the raw bytes of one
/// element. Elements are buffered one at a time and capped at
/// `MAX_JSON_ELEMENT_BYTES`; the whole document is capped at
/// `MAX_SELECTED_ENTRY_UNCOMPRESSED_BYTES`. Returns the number of elements.
///
/// Callers validate the wrapper key themselves: this grammar is shared by
/// files with different wrapper names, but an unexpected wrapper is a format
/// version problem, not a JSON syntax problem.
pub fn stream_array<R: Read>(
    reader: R,
    mut element: impl FnMut(Option<&str>, &[u8]) -> Result<(), ImportError>,
) -> Result<usize, ImportError> {
    let mut reader = BufReader::with_capacity(64 * 1024, reader);
    let mut consumed = 0u64;
    let mut prefix = Vec::new();
    let (mut in_string, mut escaped) = (false, false);
    loop {
        let byte = limited_byte(&mut reader, &mut consumed)?.ok_or_else(|| ImportError::from(ImportErrorCode::InvalidSchema))?;
        if in_string {
            prefix.push(byte);
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            // An unclosed key cannot consume an arbitrary amount of memory.
            if prefix.len() > limits::MAX_JS_PREFIX_BYTES {
                return Err(ImportErrorCode::UnsupportedFormatVersion.into());
            }
            continue;
        }
        match byte {
            b'[' => break,
            b'"' => in_string = true,
            _ => {}
        }
        prefix.push(byte);
        if prefix.len() > limits::MAX_JS_PREFIX_BYTES {
            return Err(ImportErrorCode::UnsupportedFormatVersion.into());
        }
    }
    let wrapper = wrapper_key(&prefix)?;
    let mut buffer = Vec::new();
    let (mut depth, mut in_string, mut escaped) = (1usize, false, false);
    let mut first = true;
    let mut count = 0usize;
    loop {
        let byte = limited_byte(&mut reader, &mut consumed)?.ok_or_else(|| ImportError::from(ImportErrorCode::InvalidSchema))?;
        if in_string {
            buffer.push(byte);
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
        } else if depth == 1 && buffer.is_empty() && is_whitespace(byte) {
            continue;
        } else if depth == 1 && (byte == b',' || byte == b']') {
            let ends = byte == b']';
            if trim_ascii(&buffer).is_empty() {
                // `[]` is fine; `[,` / `[1,]` / `[1,,2]` are not.
                if !(ends && first) {
                    return Err(ImportErrorCode::InvalidSchema.into());
                }
            } else {
                element(wrapper.as_deref(), &buffer)?;
                count += 1;
                if count > limits::MAX_NORMALIZED_RECORDS {
                    return Err(ImportErrorCode::ResourceLimit.into());
                }
                buffer.clear();
            }
            first = false;
            if ends {
                break;
            }
        } else {
            match byte {
                b'"' => in_string = true,
                b'{' | b'[' => {
                    depth += 1;
                    if depth + usize::from(wrapper.is_some()) > limits::MAX_JSON_NESTING {
                        return Err(ImportErrorCode::ResourceLimit.into());
                    }
                }
                b'}' if depth == 1 => return Err(ImportErrorCode::InvalidSchema.into()),
                b'}' | b']' => {
                    if depth <= 1 {
                        return Err(ImportErrorCode::InvalidSchema.into());
                    }
                    depth -= 1;
                }
                _ => {}
            }
            buffer.push(byte);
        }
        if buffer.len() > limits::MAX_JSON_ELEMENT_BYTES {
            return Err(ImportErrorCode::ResourceLimit.into());
        }
    }
    if wrapper.is_some() {
        loop {
            match limited_byte(&mut reader, &mut consumed)? {
                None => return Err(ImportErrorCode::InvalidSchema.into()),
                Some(byte) if is_whitespace(byte) => continue,
                // A comma means the top-level object has more keys than the one
                // wrapper this reader supports; that is a different shape.
                Some(b',') => return Err(ImportErrorCode::UnsupportedFormatVersion.into()),
                Some(b'}') => break,
                Some(_) => return Err(ImportErrorCode::InvalidSchema.into()),
            }
        }
    }
    loop {
        match limited_byte(&mut reader, &mut consumed)? {
            None => break,
            Some(byte) if is_whitespace(byte) => {}
            Some(_) => return Err(ImportErrorCode::InvalidSchema.into()),
        }
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn code<T: std::fmt::Debug>(value: Result<T, ImportError>) -> &'static str {
        value.expect_err("expected an error").code
    }

    #[test]
    fn stream_accepts_bom_and_rejects_bad_documents() {
        let mut values = Vec::new();
        stream_array(Cursor::new(b"\xef\xbb\xbf[1,2]"), |_, raw| { values.push(parse_element(raw)?); Ok(()) }).unwrap();
        assert_eq!(values, vec![serde_json::json!(1), serde_json::json!(2)]);
        assert_eq!(code(stream_array(Cursor::new(b"[1,2"), |_, _| Ok(()))), "invalid_schema");
        assert_eq!(code(parse_element(b"\"\xff\"")), "invalid_encoding");
    }

    #[test]
    fn stream_enforces_nesting_and_element_limits() {
        let accepted = format!("{}0{}", "[".repeat(limits::MAX_JSON_NESTING), "]".repeat(limits::MAX_JSON_NESTING));
        assert_eq!(stream_array(Cursor::new(accepted), |_, _| Ok(())).unwrap(), 1);
        let nested = format!("{}0{}", "[".repeat(limits::MAX_JSON_NESTING + 1), "]".repeat(limits::MAX_JSON_NESTING + 1));
        assert_eq!(code(stream_array(Cursor::new(nested), |_, _| Ok(()))), "resource_limit");
        let wrapped = |levels: usize| format!("{{\"k\":{}0{}}}", "[".repeat(levels), "]".repeat(levels));
        assert_eq!(stream_array(Cursor::new(wrapped(limits::MAX_JSON_NESTING - 1)), |_, _| Ok(())).unwrap(), 1);
        assert_eq!(code(stream_array(Cursor::new(wrapped(limits::MAX_JSON_NESTING)), |_, _| Ok(()))), "resource_limit");
        let mut huge = Vec::with_capacity(limits::MAX_JSON_ELEMENT_BYTES + 64);
        huge.extend_from_slice(b"[{\"x\":\"");
        huge.extend(std::iter::repeat_n(b'a', limits::MAX_JSON_ELEMENT_BYTES + 1));
        huge.extend_from_slice(b"\"}]");
        let mut parsed = 0usize;
        let error = stream_array(Cursor::new(huge), |_, _| {
            parsed += 1;
            Ok(())
        })
        .expect_err("oversized element");
        assert_eq!(error.code, "resource_limit");
        assert_eq!(parsed, 0);
    }

    #[test]
    fn unterminated_strings_hit_element_cap_before_end_of_input() {
        let mut input = b"[\"".to_vec();
        input.extend(std::iter::repeat_n(b'x', limits::MAX_JSON_ELEMENT_BYTES + 1));
        assert_eq!(code(stream_array(Cursor::new(input), |_, _| Ok(()))), "resource_limit");
    }

    #[test]
    fn stream_array_handles_bare_and_wrapped_collections() {
        let mut seen = Vec::new();
        stream_array(Cursor::new(br#" [ {"a":1}, {"b":[2]} ] "#.to_vec()), |wrapper, raw| {
            seen.push((wrapper.map(str::to_owned), std::str::from_utf8(raw).unwrap().trim().to_owned()));
            Ok(())
        })
        .unwrap();
        assert_eq!(seen, vec![
            (None, "{\"a\":1}".into()),
            (None, "{\"b\":[2]}".into()),
        ]);
        let mut seen = Vec::new();
        let count = stream_array(Cursor::new(br#"{"ig_stories":[]}"#.to_vec()), |wrapper, _| {
            seen.push(wrapper.map(str::to_owned));
            Ok(())
        })
        .unwrap();
        assert_eq!(count, 0);
        assert!(seen.is_empty());
        assert_eq!(code(stream_array(Cursor::new(br#"{"a":[],"b":[]}"#.to_vec()), |_, _| Ok(()))), "unsupported_format_version");
        assert_eq!(code(stream_array(Cursor::new(br#"[1] trailing"#.to_vec()), |_, _| Ok(()))), "invalid_schema");
        assert_eq!(code(stream_array(Cursor::new(br#"[1,]"#.to_vec()), |_, _| Ok(()))), "invalid_schema");
    }

    #[test]
    fn check_element_rejects_deep_or_large_values() {
        let accepted = serde_json::from_str::<Value>(&format!(
            "{}0{}",
            "[".repeat(limits::MAX_JSON_NESTING),
            "]".repeat(limits::MAX_JSON_NESTING)
        ))
        .unwrap();
        check_element(&accepted).expect("nesting exactly at the cap is accepted");
        let deep = serde_json::from_str::<Value>(&format!(
            "{}0{}",
            "[".repeat(limits::MAX_JSON_NESTING + 1),
            "]".repeat(limits::MAX_JSON_NESTING + 1)
        ))
        .unwrap();
        assert_eq!(check_element(&deep).expect_err("too nested").code, "resource_limit");
        let large = Value::String("a".repeat(limits::MAX_JSON_ELEMENT_BYTES + 1));
        assert_eq!(check_element(&large).expect_err("too large").code, "resource_limit");
    }


    #[test]
    fn stream_caps_raw_element_spans_not_just_reserialized_size() {
        // Each `\u0061` is six raw bytes but decodes to one byte. A single array
        // element whose reserialized form is small must still hit the raw cap.
        let escaped = "\\u0061".repeat(limits::MAX_JSON_ELEMENT_BYTES / 6 + 2);
        let document = format!("[\"{escaped}\"]");
        assert_eq!(code(stream_array(Cursor::new(document.into_bytes()), |_, _| Ok(()))), "resource_limit");
    }
}
