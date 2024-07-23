use bincode::serialize_into;

fn main() {
    let mut data: Vec<u8> = Vec::new();
    let tmp = vec![12, 13, 14, 15];

    serialize_into(&mut data, &tmp).expect("Failed to serialize into writer");
    let tmp = vec![1, 3, 2];
    serialize_into(&mut data, &tmp).expect("Failed to serialize into writer");

    let mut cursor = std::io::Cursor::new(data);
    let tmp: Vec<i32> =
        bincode::deserialize_from(&mut cursor).expect("Failed to deserialize from reader");
    println!("{:?}", tmp);
    let tmp: Vec<i32> =
        bincode::deserialize_from(&mut cursor).expect("Failed to deserialize from reader");
    println!("{:?}", tmp);
}
