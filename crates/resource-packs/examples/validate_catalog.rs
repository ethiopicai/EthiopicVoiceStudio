fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("Pass the resource catalogue path");
    let result = std::fs::read_to_string(path)
        .map_err(|_| "Could not read resource catalogue".to_string())
        .and_then(|text| resource_packs::Catalog::parse(&text))
        .and_then(|catalog| {
            if catalog.releases.is_empty() {
                Err("No public resource release is configured".into())
            } else {
                Ok(catalog)
            }
        });
    match result {
        Ok(catalog) => println!("Validated {} pinned release(s)", catalog.releases.len()),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
