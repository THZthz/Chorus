//! Tolerant reader for the CS JSON dialect: `//` and `/* */` comments,
//! trailing commas, unquoted object keys, a BOM, and raw control characters
//! inside strings. The text is normalized to strict JSON in one pass, then
//! handed to `serde_json`.

pub fn normalize(text: &str) -> String {
    let chars: Vec<char> = if text.starts_with('\u{feff}') {
        text.chars().skip(1).collect()
    } else {
        text.chars().collect()
    };
    let n = chars.len();
    let mut out: Vec<char> = Vec::with_capacity(n);
    let mut stack: Vec<char> = Vec::new();
    let mut in_str = false;
    let mut i = 0;
    while i < n {
        let c = chars[i];
        if in_str {
            if c == '\\' && i + 1 < n {
                out.push(c);
                out.push(chars[i + 1]);
                i += 2;
                continue;
            }
            if c == '"' {
                in_str = false;
                out.push(c);
                i += 1;
                continue;
            }
            if (c as u32) < 0x20 {
                out.extend(
                    match c {
                        '\n' => "\\n".chars().collect::<Vec<_>>(),
                        '\r' => "\\r".chars().collect::<Vec<_>>(),
                        '\t' => "\\t".chars().collect::<Vec<_>>(),
                        _ => "\\n".chars().collect::<Vec<_>>(),
                    }
                    .iter()
                    .copied(),
                );
                i += 1;
                continue;
            }
            out.push(c);
            i += 1;
            continue;
        }
        if c == '"' {
            in_str = true;
            out.push(c);
            i += 1;
            continue;
        }
        if c == '/' && i + 1 < n && chars[i + 1] == '/' {
            match chars[i..].iter().position(|&x| x == '\n') {
                Some(j) => i += j,
                None => i = n,
            }
            continue;
        }
        if c == '/' && i + 1 < n && chars[i + 1] == '*' {
            match chars[i + 2..].windows(2).position(|w| w == ['*', '/']) {
                Some(j) => i += j + 4,
                None => i = n,
            }
            continue;
        }
        if c == '{' || c == '[' {
            stack.push(c);
            out.push(c);
            i += 1;
            continue;
        }
        if c == '}' || c == ']' {
            stack.pop();
            out.push(c);
            i += 1;
            continue;
        }
        if c == ',' {
            let mut j = i + 1;
            while j < n && (chars[j] == ' ' || chars[j] == '\t' || chars[j] == '\r' || chars[j] == '\n') {
                j += 1;
            }
            if j < n && (chars[j] == '}' || chars[j] == ']') {
                i += 1;
                continue;
            }
            out.push(c);
            i += 1;
            continue;
        }
        if (c.is_alphabetic() || c == '_')
            && stack.last() == Some(&'{')
            && out.iter()
                .rev()
                .find(|x| !x.is_whitespace())
                .map(|&x| x == '{' || x == ',')
                .unwrap_or(false)
        {
            let mut j = i;
            while j < n && (chars[j].is_alphanumeric() || chars[j] == '_' || chars[j] == '.') {
                j += 1;
            }
            let word: String = chars[i..j].iter().collect();
            let mut k = j;
            while k < n && chars[k].is_whitespace() {
                k += 1;
            }
            if k < n && chars[k] == ':' {
                out.push('"');
                out.extend(word.chars());
                out.push('"');
                i = j;
                continue;
            }
            out.extend(word.chars());
            i = j;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out.into_iter().collect()
}

/// Parse one CS JSON file: `Ok(value)` or `Err(the parse error message)`.
pub fn parse(text: &str) -> Result<serde_json::Value, String> {
    serde_json::from_str(&normalize(text)).map_err(|e| e.to_string())
}
