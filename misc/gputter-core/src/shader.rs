#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BufferGroupBinding {
    group_index: u32,
    binding_index: u32,
}

impl std::fmt::Display for BufferGroupBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "@group({}) @binding({}) ",
            self.group_index, self.binding_index
        )
    }
}
