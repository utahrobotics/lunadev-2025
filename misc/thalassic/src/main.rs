use thalassic::Clusterer;

fn main() {
    let clusters: Vec<_> = Clusterer::new(0.4, 0.25, 4, 6, |d| (d * 3.0).round() as usize).cluster(&[
        0.0, 0.1, 0.2, 0.3,
        0.4, 0.5, 0.6, 0.7,
        0.8, 0.9, 1.0, 1.1,
        1.2, 1.3, 1.4, 1.5,
    ]).collect();
    println!("{:#?}", clusters);
}