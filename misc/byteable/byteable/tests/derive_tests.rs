use byteable::{EmptyVec, FillByteVec, IntoBytes};

#[derive(IntoBytes)]
struct FourZeroes;

impl FillByteVec for FourZeroes {
    fn fill_bytes(&self, vec: EmptyVec<u8>) {
        let vec: &mut Vec<u8> = vec.into();
        vec.extend_from_slice(&[0, 0, 0, 0]);
    }
}

#[test]
fn test_four_zeroes() {
    let src = FourZeroes;
    assert_eq!(&[0, 0, 0, 0], src.to_bytes().as_slice());
}

#[derive(IntoBytes)]
struct FourEights;

impl FillByteVec for FourEights {
    fn fill_bytes(&self, vec: EmptyVec<u8>) {
        let vec: &mut Vec<u8> = vec.into();
        vec.extend_from_slice(&[8, 8, 8, 8]);
    }
}

#[test]
fn test_four_eights() {
    let src = FourEights;
    assert_eq!(&[8, 8, 8, 8], src.to_bytes().as_slice());
}

#[test]
fn test_eight_then_zero() {
    let src = FourEights;
    assert_eq!(&[8, 8, 8, 8], src.to_bytes().as_slice());
    let src = FourZeroes;
    assert_eq!(&[0, 0, 0, 0], src.to_bytes().as_slice());
}
