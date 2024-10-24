use gputter_macros::build_shader;
build_shader!(
    Test,
    r#"
{{BUFFER}} var<storage, read_write> heightmap: array<atomic<u32>, 3>;
{{BUFFER}} var<storage, read> points: array<vec4<f32>, 3>;
{{BUFFER}} var<storage, read> original_heightmap: array<f32, 4>;

const NUMBER: f32 = {{number}};

@compute
@workgroup_size(1, 1, 1)
fn main(
    @builtin(workgroup_id) workgroup_id : vec3<u32>,
) {}"#
);

fn main() {
    let test = Test {
        heightmap: todo!(),
        points: todo!(),
        original_heightmap: todo!(),
        number: 0.2,
    };
    test.compile();
}
