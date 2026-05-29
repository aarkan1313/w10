//! Minimal NumPy-v1.0 `.npy` reader. The only byte-parser besides JSON
//! (DESIGN §3). Pure: no `godot` imports. Supports 2-D C-order `<f4`/`<f8`
//! arrays (what `np.save` writes by default); rejects anything else.

/// A parsed 2-D float kernel: row-major `data`, `rows` x `cols`, as f32.
#[derive(Debug, Clone, PartialEq)]
pub struct Kernel {
    pub rows: usize,
    pub cols: usize,
    pub data: Vec<f32>,
}

const MAGIC: &[u8] = b"\x93NUMPY";

/// Parse a `.npy` byte buffer into a `Kernel`. Returns a descriptive error on
/// any unsupported/malformed input (no silent defaults).
pub fn read_npy_f32(bytes: &[u8]) -> Result<Kernel, String> {
    if bytes.len() < 10 || &bytes[0..6] != MAGIC {
        return Err("npy: bad magic (not a NumPy .npy file)".to_string());
    }
    let major = bytes[6];
    if major != 1 {
        return Err(format!("npy: unsupported version {major}.x (only v1 supported)"));
    }
    let header_len = u16::from_le_bytes([bytes[8], bytes[9]]) as usize;
    let header_start = 10;
    let data_start = header_start + header_len;
    if bytes.len() < data_start {
        return Err("npy: truncated header".to_string());
    }
    let header = std::str::from_utf8(&bytes[header_start..data_start])
        .map_err(|_| "npy: non-utf8 header".to_string())?;

    let descr = extract_value(header, "descr")
        .ok_or_else(|| "npy: header missing descr".to_string())?;
    let elem_size = match descr.as_str() {
        "<f4" => 4usize,
        "<f8" => 8usize,
        other => return Err(format!("npy: unsupported dtype/descr {other:?} (only <f4/<f8)")),
    };

    let fortran = extract_value(header, "fortran_order")
        .ok_or_else(|| "npy: header missing fortran_order".to_string())?;
    if fortran != "False" {
        return Err("npy: fortran_order=True unsupported (C-order only)".to_string());
    }

    let (rows, cols) = parse_shape(header)?;
    let count = rows
        .checked_mul(cols)
        .ok_or_else(|| "npy: shape too large (rows*cols overflow)".to_string())?;
    let need = count
        .checked_mul(elem_size)
        .ok_or_else(|| "npy: shape too large (byte count overflow)".to_string())?;
    if bytes.len() < data_start + need {
        return Err(format!(
            "npy: data too short: need {} bytes for {}x{} {}, have {}",
            need, rows, cols, descr, bytes.len() - data_start
        ));
    }

    let raw = &bytes[data_start..data_start + need];
    let mut data = Vec::with_capacity(count);
    if elem_size == 4 {
        for chunk in raw.chunks_exact(4) {
            data.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
        }
    } else {
        for chunk in raw.chunks_exact(8) {
            let v = f64::from_le_bytes([
                chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
            ]);
            data.push(v as f32);
        }
    }
    Ok(Kernel { rows, cols, data })
}

/// Pull a value for `key` out of the python-dict-ish header. Handles
/// `'key': value` where value is a quoted string (`'<f4'`) or a bareword
/// (`False`). Returns the value without surrounding quotes.
fn extract_value(header: &str, key: &str) -> Option<String> {
    let needle = format!("'{key}':");
    let i = header.find(&needle)? + needle.len();
    let rest = header[i..].trim_start();
    if let Some(stripped) = rest.strip_prefix('\'') {
        let end = stripped.find('\'')?;
        Some(stripped[..end].to_string())
    } else {
        let end = rest.find(|c: char| c == ',' || c == '}' || c.is_whitespace())
            .unwrap_or(rest.len());
        Some(rest[..end].to_string())
    }
}

/// Parse `'shape': (rows, cols)` (exactly 2-D). 1-D or N-D is rejected.
fn parse_shape(header: &str) -> Result<(usize, usize), String> {
    let needle = "'shape':";
    let i = header.find(needle).ok_or_else(|| "npy: header missing shape".to_string())? + needle.len();
    let rest = &header[i..];
    let open = rest.find('(').ok_or_else(|| "npy: malformed shape".to_string())?;
    let close = rest[open..].find(')').ok_or_else(|| "npy: malformed shape".to_string())? + open;
    let inner = &rest[open + 1..close];
    let dims: Vec<usize> = inner
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<usize>().map_err(|_| format!("npy: bad shape dim {s:?}")))
        .collect::<Result<_, _>>()?;
    if dims.len() != 2 {
        return Err(format!("npy: shape must be 2-D, got {}-D", dims.len()));
    }
    if dims[0] == 0 || dims[1] == 0 {
        return Err("npy: shape has a zero dimension".to_string());
    }
    Ok((dims[0], dims[1]))
}
