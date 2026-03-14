/// ABI version exposed to plugins.
pub const ABI_VERSION: &str = "0.1";

/// WASM module namespace for all host imports.
pub const HOST_MODULE: &str = "rustflow";

/// Function names the plugin must export.
pub const FN_ALLOC: &str = "rustflow_alloc";
pub const FN_DEALLOC: &str = "rustflow_dealloc";
pub const FN_MANIFEST: &str = "rustflow_plugin_manifest";
pub const FN_EXECUTE: &str = "rustflow_tool_execute";

/// WASM linear memory export name.
pub const MEMORY: &str = "memory";

/// Pack a (ptr, len) pair into an i64 for return across the WASM boundary.
///
/// The upper 32 bits hold the pointer; the lower 32 bits hold the length.
/// Both values are treated as unsigned 32-bit integers.
#[inline]
pub fn pack_ptr_len(ptr: u32, len: u32) -> i64 {
    (((ptr as u64) << 32) | (len as u64)) as i64
}

/// Unpack an i64 returned from a WASM function into (ptr, len).
///
/// Interprets the raw bits as an unsigned 64-bit integer before masking.
#[inline]
pub fn unpack_ptr_len(packed: i64) -> (u32, u32) {
    let u = packed as u64;
    let ptr = (u >> 32) as u32;
    let len = (u & 0xFFFF_FFFF) as u32;
    (ptr, len)
}

/// Read a UTF-8 string from WASM linear memory at the given offset and length.
pub fn read_str(data: &[u8], ptr: u32, len: u32) -> crate::error::Result<String> {
    let start = ptr as usize;
    let end = start
        .checked_add(len as usize)
        .filter(|&e| e <= data.len())
        .ok_or_else(|| crate::error::PluginError::AbiViolation {
            reason: format!(
                "read_str: ptr={ptr}, len={len} overflows memory (size={})",
                data.len()
            ),
        })?;
    String::from_utf8(data[start..end].to_vec()).map_err(|e| crate::error::PluginError::AbiViolation {
        reason: format!("read_str: invalid UTF-8 at ptr={ptr}: {e}"),
    })
}

/// Write bytes into WASM linear memory at the given offset.
pub fn write_bytes(data: &mut [u8], ptr: u32, bytes: &[u8]) -> crate::error::Result<()> {
    let start = ptr as usize;
    let end = start
        .checked_add(bytes.len())
        .filter(|&e| e <= data.len())
        .ok_or_else(|| crate::error::PluginError::AbiViolation {
            reason: format!(
                "write_bytes: ptr={ptr}, len={} overflows memory (size={})",
                bytes.len(),
                data.len()
            ),
        })?;
    data[start..end].copy_from_slice(bytes);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pack_unpack_roundtrip() {
        let cases: &[(u32, u32)] = &[
            (0, 0),
            (0, 42),
            (4096, 128),
            (u32::MAX, 0),
            (0, u32::MAX),
            (0x8000_0000, 0x8000_0000),
        ];
        for &(ptr, len) in cases {
            let packed = pack_ptr_len(ptr, len);
            let (p2, l2) = unpack_ptr_len(packed);
            assert_eq!((p2, l2), (ptr, len), "ptr={ptr}, len={len}");
        }
    }

    #[test]
    fn test_read_str_valid() {
        let data = b"hello world";
        assert_eq!(read_str(data, 0, 5).unwrap(), "hello");
        assert_eq!(read_str(data, 6, 5).unwrap(), "world");
    }

    #[test]
    fn test_read_str_overflow() {
        let data = b"hi";
        assert!(read_str(data, 1, 5).is_err());
    }

    #[test]
    fn test_write_bytes() {
        let mut data = vec![0u8; 16];
        write_bytes(&mut data, 4, b"ABCD").unwrap();
        assert_eq!(&data[4..8], b"ABCD");
    }

    #[test]
    fn test_write_bytes_overflow() {
        let mut data = vec![0u8; 4];
        assert!(write_bytes(&mut data, 3, b"ABCD").is_err());
    }
}
