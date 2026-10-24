//! Reconstruct the waiting UI from the game's archive and the embedded Training UI.
//! The recipe contains only copy ranges and changed bytes, not a second game archive.
use std::convert::TryInto;

type Result<T> = std::result::Result<T, &'static str>;

// The validated character + 115 BC7 stage portraits produce an 8,509,244-byte
// archive. Keep a bounded allocation above that payload; RGBA8 stage imports
// (13,453,116 bytes) remain rejected.
pub const MAX_WAITING_UI_BYTES: usize = 9 * 1024 * 1024;

fn take<'a>(bytes: &mut &'a [u8], len: usize) -> Result<&'a [u8]> {
    let value = bytes.get(..len).ok_or("truncated waiting UI patch")?;
    *bytes = &bytes[len..];
    Ok(value)
}

fn read<const N: usize>(bytes: &mut &[u8]) -> Result<[u8; N]> {
    Ok(take(bytes, N)?.try_into().expect("fixed-width field"))
}

// An identity check for versioned resources, not a cryptographic authenticity check.
fn fingerprint(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

pub fn build(stock: &[u8], training: &[u8], mut patch: &[u8]) -> Result<Vec<u8>> {
    if read::<4>(&mut patch)? != *b"TMW1" {
        return Err("unsupported waiting UI patch");
    }
    for (source, error) in [
        (stock, "waiting UI stock resource mismatch"),
        (training, "waiting UI Training resource mismatch"),
    ] {
        let len = u32::from_le_bytes(read(&mut patch)?) as usize;
        let hash = u64::from_le_bytes(read(&mut patch)?);
        if source.len() != len || fingerprint(source) != hash {
            return Err(error);
        }
    }
    let size = u32::from_le_bytes(read(&mut patch)?) as usize;
    let hash = u64::from_le_bytes(read(&mut patch)?);
    if !(4..=MAX_WAITING_UI_BYTES).contains(&size) {
        return Err("invalid waiting UI output size");
    }
    let mut output = Vec::with_capacity(size);
    while !patch.is_empty() {
        let [source] = read(&mut patch)?;
        let len = u32::from_le_bytes(read(&mut patch)?) as usize;
        if len > size - output.len() {
            return Err("waiting UI output exceeds declared size");
        }
        let bytes = match source {
            0 | 1 => {
                let offset = u32::from_le_bytes(read(&mut patch)?) as usize;
                [stock, training][usize::from(source)]
                    .get(offset..)
                    .and_then(|bytes| bytes.get(..len))
                    .ok_or("waiting UI copy outside source")?
            }
            2 => take(&mut patch, len)?,
            _ => return Err("invalid waiting UI patch operation"),
        };
        output.extend_from_slice(bytes);
    }
    if output.len() != size || fingerprint(&output) != hash {
        return Err("waiting UI reconstruction mismatch");
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    // A hand-written three-operation recipe: copy two inputs and insert changed bytes.
    fn fixture() -> (Vec<u8>, Vec<u8>) {
        let expected = b"waiting + training";
        let mut patch = b"TMW1".to_vec();
        for bytes in [b"waiting".as_slice(), b"training", expected] {
            patch.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            patch.extend_from_slice(&fingerprint(bytes).to_le_bytes());
        }
        patch.extend_from_slice(&[0, 7, 0, 0, 0, 0, 0, 0, 0]);
        patch.extend_from_slice(&[2, 3, 0, 0, 0, b' ', b'+', b' ']);
        patch.extend_from_slice(&[1, 8, 0, 0, 0, 0, 0, 0, 0]);
        (patch, expected.to_vec())
    }

    #[test]
    fn reconstructs_both_sources_and_changed_bytes() {
        let (patch, expected) = fixture();
        assert_eq!(build(b"waiting", b"training", &patch), Ok(expected));
    }

    #[test]
    fn rejects_changed_game_or_embedded_resources() {
        let (patch, _) = fixture();
        assert!(build(b"waitinG", b"training", &patch).is_err());
        assert!(build(b"waiting", b"Training", &patch).is_err());
        assert!(build(b"waiting!", b"training", &patch).is_err());
    }

    #[test]
    fn never_accepts_partial_or_corrupted_recipes() {
        let (patch, _) = fixture();
        for end in 0..patch.len() {
            assert!(build(b"waiting", b"training", &patch[..end]).is_err());
        }
        for offset in [0, 4, 8, 16, 20, 28, 32, 40, 41, 45, 49, 54, 57, 62] {
            let mut corrupt = patch.clone();
            corrupt[offset] ^= 0x80;
            assert!(
                build(b"waiting", b"training", &corrupt).is_err(),
                "accepted corruption at {}",
                offset
            );
        }
    }
}
