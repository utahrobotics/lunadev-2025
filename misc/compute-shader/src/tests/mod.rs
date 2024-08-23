use wgpu::include_wgsl;

use crate::{
    buffers::{BufferType, HostReadWrite, ShaderReadWrite},
    Compute,
};

#[tokio::test]
async fn mul2() {
    env_logger::init();
    let compute = Compute::<(BufferType<[[u32; 4]], HostReadWrite, ShaderReadWrite>,)>::new(
        include_wgsl!("mul2.wgsl"),
        BufferType::new_dyn(5),
    )
    .await
    .unwrap();

    let original = [
        [1, 2, 3, 1],
        [4, 5, 6, 3],
        [7, 8, 9, 5],
        [10, 11, 12, 2],
        [13, 14, 15, 2],
    ];
    let mut vecs = original;
    compute
        .new_pass(vecs.as_slice())
        .workgroups_count(5, 1, 1)
        .call(vecs.as_mut_slice())
        .await;

    for (input, output) in original.into_iter().zip(vecs.into_iter()) {
        assert_eq!(
            [input[0] * 2, input[1] * 2, input[2] * 2, input[3]],
            [output[0], output[1], output[2], output[3]]
        );
    }
}
