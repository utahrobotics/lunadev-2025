use std::future::Future;

use common::{FromLunabase, FromLunabot};

pub trait TeleOpComponent {
    fn from_lunabase(&mut self) -> impl Future<Output = FromLunabase>;
    fn to_lunabase_unreliable(&mut self, to_lunabase: FromLunabot) -> impl Future<Output = ()>;
    fn to_lunabase_reliable(&mut self, to_lunabase: FromLunabot) -> impl Future<Output = ()>;
}
