use image::imageops::FilterType;
use ico::{IconDir, IconDirEntry, IconImage, ResourceType};
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

const ICON_SIZES: &[u32] = &[16, 24, 32, 48, 64, 128, 256];

fn main() {
    let icon_png = Path::new("icon.png");
    let icon_ico = Path::new("icon.ico");

    println!("cargo:rerun-if-changed=icon.png");

    if icon_png.exists() {
        generate_ico(icon_png, icon_ico);
        embed_windows_icon(icon_ico);
    }
}

fn generate_ico(png_path: &Path, ico_path: &Path) {
    let source = image::open(png_path).expect("failed to open icon.png");

    let mut icon_dir = IconDir::new(ResourceType::Icon);

    for &size in ICON_SIZES {
        let resized = source.resize_exact(size, size, FilterType::Lanczos3);
        let rgba = resized.to_rgba8();
        let icon_image =
            IconImage::from_rgba_data(size, size, rgba.into_raw());
        icon_dir.add_entry(IconDirEntry::encode(&icon_image).expect("failed to encode icon entry"));
    }

    let file = File::create(ico_path).expect("failed to create icon.ico");
    icon_dir
        .write(BufWriter::new(file))
        .expect("failed to write icon.ico");
}

fn embed_windows_icon(ico_path: &Path) {
    let mut res = winres::WindowsResource::new();
    res.set_icon(ico_path.to_str().expect("icon path is not valid UTF-8"));
    res.compile().expect("failed to compile Windows resource");
}
