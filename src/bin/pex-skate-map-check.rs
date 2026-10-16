use skate_data::skate_map::SkateMap;
use skate_host::bridge::Session;
use std::path::PathBuf;

fn argument(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<PathBuf, String> {
    let value = args
        .next()
        .ok_or_else(|| format!("missing value after {flag}"))?;
    Ok(PathBuf::from(value))
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mut assets = None;
    let mut map_path = None;
    let mut check_assets = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--assets" => assets = Some(argument(&mut args, "--assets")?),
            "--map" => map_path = Some(argument(&mut args, "--map")?),
            "--check-assets" => check_assets = true,
            _ => return Err(format!("unexpected argument: {arg}")),
        }
    }
    if !check_assets {
        return Err("--check-assets is required".into());
    }
    let assets = assets.ok_or("--assets is required")?;
    let map_path = map_path.ok_or("--map is required")?;
    let map = SkateMap::load(&map_path)?;
    let session = Session::new_with_skate_map(&assets, &map)
        .map_err(|error| format!("{}: {error}", map_path.display()))?;
    drop(session);
    println!(
        "Validated Skate map {:?}: {} render triangles, {} rails, spawn {:?}",
        map.name,
        map.geometry.indices.len() / 3,
        map.rails.len(),
        map.spawn
    );
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Skate map check failed: {error}");
        std::process::exit(1);
    }
}
