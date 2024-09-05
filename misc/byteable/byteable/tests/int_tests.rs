use byteable::IntoBytes;

#[test]
fn test_u32() {
    let src = 215312u32;
    assert_eq!(src.to_ne_bytes().as_slice(), &*src.to_bytes());
}
