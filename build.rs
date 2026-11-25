use std::{env, fs, io, path::PathBuf};

fn main() -> io::Result<()> {
    let icon_path = "assets/icon.ico";
    let manifest_path = "assets/app.manifest";
    println!("cargo:rerun-if-changed={icon_path}");
    println!("cargo:rerun-if-changed={manifest_path}");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
        || env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc")
    {
        return Ok(());
    }

    // Emit Windows resources directly so a build also works with rust-lld.
    let icon = fs::read(icon_path)?;
    let header = icon
        .get(..6)
        .ok_or_else(|| io::Error::other("Invalid app icon."))?;
    let count = u16::from_le_bytes([header[4], header[5]]);
    let mut resources = Vec::new();
    resource(&mut resources, 0, 0, 0, &[]);
    let mut group = header.to_vec();
    for index in 0..count {
        let start = 6 + usize::from(index) * 16;
        let entry = icon
            .get(start..start + 16)
            .ok_or_else(|| io::Error::other("Invalid app icon entry."))?;
        let size = u32::from_le_bytes(entry[8..12].try_into().unwrap()) as usize;
        let offset = u32::from_le_bytes(entry[12..16].try_into().unwrap()) as usize;
        let image = icon
            .get(offset..offset.saturating_add(size))
            .ok_or_else(|| io::Error::other("Invalid app icon image."))?;
        let id = index + 1;
        resource(&mut resources, 3, id, 1033, image);
        group.extend_from_slice(&entry[..12]);
        group.extend_from_slice(&id.to_le_bytes());
    }
    resource(&mut resources, 14, 1, 1033, &group);
    resource(&mut resources, 24, 1, 1033, &fs::read(manifest_path)?);
    let path = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("aztromidi.res");
    fs::write(&path, resources)?;
    println!("cargo:rustc-link-arg-bin=aztromidi={}", path.display());
    Ok(())
}

fn resource(out: &mut Vec<u8>, kind: u16, id: u16, language: u16, data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&32u32.to_le_bytes());
    for word in [0xffff, kind, 0xffff, id, 0, 0, 0, language, 0, 0, 0, 0u16] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    out.extend_from_slice(data);
    out.resize(out.len().next_multiple_of(4), 0);
}
