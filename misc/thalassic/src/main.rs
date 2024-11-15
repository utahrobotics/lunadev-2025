use thalassic::Clusterer;

fn main() {
    let img = image::load_from_memory(include_bytes!("heatmap.png")).unwrap().to_luma32f();
    let heatmap: Vec<_> = img.into_vec().into_iter().map(|n| n as f64).collect();
    let clusters: Vec<_> = Clusterer::new(0.1, 0.05, 64, 2, |d| (d * 3.0).round() as usize).cluster(&heatmap).collect();
    
    // (0.6, 0.5)
    // (1.5, 1.15)
    // (1.85, 0.25)
    // (2.6, 0.85)

    println!("{:#?}", clusters);
}