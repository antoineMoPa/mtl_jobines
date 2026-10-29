use super::Result;
use resvg::{tiny_skia, usvg};
use std::{fs, path::Path};

pub(super) fn generate(root: &Path) -> Result<()> {
    let mut options = usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    for name in ["Georgia", "Arial"] {
        let query = usvg::fontdb::Query {
            families: &[usvg::fontdb::Family::Name(name)],
            ..Default::default()
        };
        if options.fontdb.query(&query).is_none() {
            return Err(format!("Install {name} before running cargo xtask assets.").into());
        }
    }
    let directory = root.join("assets/commerce");
    let mut sources = vec![directory.join("interiors.svg")];
    for entry in fs::read_dir(directory.join("brands"))? {
        let path = entry?.path();
        if path.extension().is_some_and(|extension| extension == "svg") {
            sources.push(path);
        }
    }
    sources.sort();
    let mut written = 0;
    for source in &sources {
        let png = rasterize(&fs::read(source)?, &options)?;
        let destination = source.with_extension("png");
        if fs::read(&destination).ok().as_deref() != Some(png.as_slice()) {
            fs::write(destination, png)?;
            written += 1;
        }
    }
    println!(
        "Generated {} commerce PNGs ({written} updated).",
        sources.len()
    );
    Ok(())
}

fn rasterize(svg: &[u8], options: &usvg::Options) -> Result<Vec<u8>> {
    let tree = usvg::Tree::from_data(svg, options)?;
    let size = tree.size().to_int_size();
    let mut pixmap =
        tiny_skia::Pixmap::new(size.width(), size.height()).ok_or("Cannot allocate SVG raster")?;
    resvg::render(
        &tree,
        tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    Ok(pixmap.encode_png()?)
}
