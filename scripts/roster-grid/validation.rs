use crate::Result;
use nx_layout_toolbox::bntx::BntxFile;
pub fn validate_texture_dictionary(file: &BntxFile) -> Result<()> {
    for (index, texture) in file.textures.iter().enumerate() {
        let name = texture.name(file);
        let entries = &file.dict.entries;
        let mut node = usize::from(entries.first().ok_or("empty texture dictionary")?.left);
        let mut resolved = None;
        for _ in 0..=entries.len() {
            let current = entries.get(node).ok_or("dictionary child out of range")?;
            let bit = current.ref_bit as usize;
            let value = if bit < name.len() * 8 {
                (name.as_bytes()[name.len() - 1 - bit / 8] >> (bit % 8)) & 1
            } else {
                0
            };
            let next = usize::from(if value == 0 {
                current.left
            } else {
                current.right
            });
            let next_entry = entries.get(next).ok_or("dictionary child out of range")?;
            let signed_bit = |v: u32| if v == u32::MAX { -1 } else { i64::from(v) };
            if signed_bit(next_entry.ref_bit) <= signed_bit(current.ref_bit) {
                resolved = Some(next);
                break;
            }
            node = next;
        }
        if resolved != Some(index + 1) {
            return Err(format!(
                "texture dictionary misroutes {name}: {resolved:?}, expected {}",
                index + 1
            )
            .into());
        }
        let entry = &entries[index + 1];
        if file
            .strings
            .get(entry.string_index as usize)
            .map(String::as_str)
            != Some(name)
        {
            return Err(format!("texture dictionary string differs: {name}").into());
        }
    }
    Ok(())
}
