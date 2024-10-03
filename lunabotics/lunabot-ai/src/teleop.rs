use std::future::Future;

use common::{FromLunabase, FromLunabot};

pub trait TeleOp {
    fn from_lunabase(&mut self) -> impl Future<Output = FromLunabase>;
    fn to_lunabase(&mut self, to_lunabase: FromLunabot) -> impl Future<Output = ()>;
}
