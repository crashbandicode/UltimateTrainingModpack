//! Host-side repair for the toolbox writer's proven one-pointer-per-BRTI limit.
//! Original stock/donor mip tables are the oracle; no mip offsets are inferred
//! from dimensions or compressed pixel sizes.
use crate::Result;
use std::collections::BTreeMap;

fn u16_at(data: &[u8], at: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        data.get(at..at + 2)
            .ok_or("u16 out of bounds")?
            .try_into()?,
    ))
}
fn u32_at(data: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        data.get(at..at + 4)
            .ok_or("u32 out of bounds")?
            .try_into()?,
    ))
}
fn u64_at(data: &[u8], at: usize) -> Result<usize> {
    Ok(usize::try_from(u64::from_le_bytes(
        data.get(at..at + 8)
            .ok_or("u64 out of bounds")?
            .try_into()?,
    ))?)
}
fn put32(data: &mut [u8], at: usize, value: usize) -> Result<()> {
    data.get_mut(at..at + 4)
        .ok_or("u32 write out of bounds")?
        .copy_from_slice(&u32::try_from(value)?.to_le_bytes());
    Ok(())
}
fn put64(data: &mut [u8], at: usize, value: usize) -> Result<()> {
    data.get_mut(at..at + 8)
        .ok_or("u64 write out of bounds")?
        .copy_from_slice(&(value as u64).to_le_bytes());
    Ok(())
}

#[derive(Debug)]
struct Texture {
    name: String,
    brti: usize,
    pointer_table: usize,
    pixel: usize,
    mip_count: usize,
    image_size: usize,
}

fn textures(data: &[u8]) -> Result<Vec<Texture>> {
    if !data.starts_with(b"BNTX") {
        return Err("not BNTX".into());
    }
    let count = u32_at(data, 0x24)? as usize;
    let pointers = u64_at(data, 0x28)?;
    (0..count)
        .map(|i| {
            let brti = u64_at(data, pointers + i * 8)?;
            if data.get(brti..brti + 4) != Some(b"BRTI") {
                return Err("bad BRTI pointer".into());
            }
            let name_at = u64_at(data, brti + 0x60)?;
            let name_len = u16_at(data, name_at)? as usize;
            let name = String::from_utf8(
                data.get(name_at + 2..name_at + 2 + name_len)
                    .ok_or("name out of bounds")?
                    .to_vec(),
            )?;
            let pointer_table = u64_at(data, brti + 0x70)?;
            Ok(Texture {
                name,
                brti,
                pointer_table,
                pixel: u64_at(data, pointer_table)?,
                mip_count: u16_at(data, brti + 0x16)? as usize,
                image_size: u32_at(data, brti + 0x50)? as usize,
            })
        })
        .collect()
}

pub fn mip_offsets(data: &[u8]) -> Result<BTreeMap<String, Vec<usize>>> {
    let mut result = BTreeMap::new();
    for texture in textures(data)? {
        let offsets = (0..texture.mip_count)
            .map(|mip| {
                let ptr = u64_at(data, texture.pointer_table + mip * 8)?;
                let relative = ptr
                    .checked_sub(texture.pixel)
                    .ok_or_else(|| format!("{} mip{mip} precedes base", texture.name))?;
                if relative >= texture.image_size {
                    return Err(format!("{} mip{mip} exceeds pixel payload", texture.name).into());
                }
                Ok(relative)
            })
            .collect::<Result<Vec<_>>>()?;
        if offsets.first() != Some(&0) || offsets.windows(2).any(|w| w[0] >= w[1]) {
            return Err(format!("invalid mip offsets: {}", texture.name).into());
        }
        result.insert(texture.name, offsets);
    }
    Ok(result)
}

fn entry(position: usize, structs: usize, offsets: usize, pad: usize) -> Result<[u8; 8]> {
    let mut row = [0; 8];
    row[..4].copy_from_slice(&u32::try_from(position)?.to_le_bytes());
    row[4..6].copy_from_slice(&u16::try_from(structs)?.to_le_bytes());
    row[6] = u8::try_from(offsets)?;
    row[7] = u8::try_from(pad)?;
    Ok(row)
}

pub fn repair(data: &[u8], stock: &[u8]) -> Result<Vec<u8>> {
    let mut oracle = mip_offsets(stock)?;
    let textures = textures(data)?;
    for texture in &textures {
        if !oracle.contains_key(&texture.name) {
            if texture.mip_count != 1 {
                return Err(format!("new texture {} requires one mip", texture.name).into());
            }
            oracle.insert(texture.name.clone(), vec![0]);
        }
    }
    let first = textures.first().ok_or("empty BNTX")?.brti;
    let old_brtd = u64_at(data, 0x30)?;
    let old_rlt = u32_at(data, 0x18)? as usize;
    let mut out = data.get(..first).ok_or("metadata out of bounds")?.to_vec();
    let mut positions = Vec::new();
    let mut entries0 = vec![
        entry(0x28, 1, 1, 0)?,
        entry(0x38, 1, 2, 0)?,
        entry(u64_at(data, 0x28)?, textures.len(), 1, 0)?,
        entry(u64_at(data, 0x38)? + 16, textures.len() + 1, 1, 1)?,
    ];
    let mut entries1 = vec![entry(0x30, 1, 1, 0)?];
    for texture in &textures {
        let offsets = oracle.get(&texture.name).ok_or("missing mip oracle")?;
        if offsets.len() != texture.mip_count || texture.pointer_table != texture.brti + 0x2a0 {
            return Err(format!("unexpected source mip layout: {}", texture.name).into());
        }
        let start = out.len();
        positions.push(start);
        // BRTI + GPU object storage are copied verbatim. Only the variable mip
        // pointer array following them grows, matching original donor BRTIs.
        out.extend_from_slice(
            data.get(texture.brti..texture.brti + 0x2a0)
                .ok_or("BRTI out of bounds")?,
        );
        out.resize(out.len() + offsets.len() * 8, 0);
        put64(&mut out, start + 0x70, start + 0x2a0)?;
        put64(&mut out, start + 0x80, start + 0xa0)?;
        put64(&mut out, start + 0x88, start + 0x1a0)?;
        entries0.push(entry(start + 0x60, 1, 3, 0)?);
        entries0.push(entry(start + 0x80, 1, 2, 0)?);
        entries1.push(entry(start + 0x2a0, 1, offsets.len(), 0)?);
    }
    let metadata_end = out.len();
    let alignment = 1usize << data[0xe];
    let pixel_start = (metadata_end + 16 + alignment - 1) & !(alignment - 1);
    let brtd = pixel_start - 16;
    out.resize(brtd, 0);
    out.extend_from_slice(data.get(old_brtd..old_rlt).ok_or("BRTD out of bounds")?);
    // The GPU pool needs an aligned extent as well as an aligned start. Owned
    // stock/donor payloads are 0xef000/0x3a4000 (both 4096-aligned). Appending
    // 512-aligned textures left our first merged payload at 0x460a00. Include
    // zero tail padding in BRTD's declared size and the target RLT section size.
    let rlt = (out.len() + alignment - 1) & !(alignment - 1);
    out.resize(rlt, 0);
    put64(&mut out, brtd + 8, rlt - brtd)?;
    for (i, texture) in textures.iter().enumerate() {
        let start = positions[i];
        let next = positions.get(i + 1).copied().unwrap_or(brtd);
        put32(&mut out, start + 4, next - start)?;
        put64(&mut out, start + 8, next - start)?;
        put64(&mut out, u64_at(data, 0x28)? + i * 8, start)?;
        let relative_pixel = texture
            .pixel
            .checked_sub(old_brtd + 16)
            .ok_or("pixel precedes BRTD")?;
        for (mip, relative) in oracle[&texture.name].iter().enumerate() {
            put64(
                &mut out,
                start + 0x2a0 + mip * 8,
                pixel_start + relative_pixel + relative,
            )?;
        }
    }
    put64(&mut out, 0x30, brtd)?;
    put32(&mut out, 0x18, rlt)?;
    out.extend_from_slice(b"_RLT");
    out.extend_from_slice(&u32::try_from(rlt)?.to_le_bytes());
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    for (pos, size, index, count) in [
        (0, metadata_end, 0, entries0.len()),
        (brtd, rlt - brtd, entries0.len(), entries1.len()),
    ] {
        out.extend_from_slice(&0u64.to_le_bytes());
        for field in [pos, size, index, count] {
            out.extend_from_slice(&u32::try_from(field)?.to_le_bytes());
        }
    }
    for row in entries0.iter().chain(&entries1) {
        out.extend_from_slice(row);
    }
    let size = out.len();
    put32(&mut out, 0x1c, size)?;
    let verified = mip_offsets(&out)?;
    for texture in &textures {
        if verified.get(&texture.name) != oracle.get(&texture.name) {
            return Err(format!("mip repair verification failed: {}", texture.name).into());
        }
    }
    Ok(out)
}
