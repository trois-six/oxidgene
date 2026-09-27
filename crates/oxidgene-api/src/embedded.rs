//! Data embedded into the binary, Brotli-compressed at build time (see
//! `build.rs`) or by `just places` (`assets/places/`).

use std::io::Read;

/// Decompresses embedded bytes. They are part of the binary, so a stream that
/// does not decode is a build defect, not a runtime condition.
pub(crate) fn decompress(compressed: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    brotli_decompressor::Decompressor::new(compressed, 1 << 16)
        .read_to_end(&mut bytes)
        .expect("embedded data must be a valid Brotli stream");
    bytes
}

#[cfg(test)]
mod tests {
    use super::decompress;

    use std::io::Write;

    #[test]
    fn decodes_what_the_build_script_encodes() {
        let text = "Paroisse A, Paroisse B, Paroisse A, Paroisse B".repeat(100);
        let mut compressed = Vec::new();
        {
            let mut encoder = brotli::CompressorWriter::new(&mut compressed, 1 << 16, 11, 24);
            encoder.write_all(text.as_bytes()).unwrap();
        }
        assert!(compressed.len() < text.len() / 10);
        assert_eq!(decompress(&compressed), text.as_bytes());
    }
}
