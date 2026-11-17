//! Offline authoring of ordinary .road building geometry.
mod density;
use std::{error::Error, time::Instant};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let started = Instant::now();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["pack", directory] => density::build(directory)?,
        ["fill", directory] => density::fill(directory)?,
        ["inner-loop", directory] => density::inner_loop(directory)?,
        ["extract-green", pbf, calibration, output] => {
            density::export_green(pbf, calibration, output)?
        }
        _ => {
            return Err(
                "Use pack DIR, fill DIR, inner-loop DIR, or extract-green PBF CALIBRATION OUTPUT"
                    .into(),
            )
        }
    }
    eprintln!("Finished in {:.2}s", started.elapsed().as_secs_f64());
    Ok(())
}
