use std::convert::TryInto;

use iceoryx2::{prelude::*, port::publisher::Publisher};

use crate::{RECORDER_SERVICE_PATH, RerunMessage};

pub struct Recorder<const N: usize> {
    publisher: Publisher<Service, RerunMessage<N>, ()>
}

pub enum RerunLevel {
    All,
    Minimal
}

impl<const N: usize> Recorder<N> {
    fn new(log_level: RerunLevel) -> Result<Self, Box<dyn core::error::Error>> {
        let node = NodeBuilder::new().name(&"rerun_recorder".try_into()?).create::<ipc::Service>()?;
        let service = node
            .service_builder(&RECORDER_SERVICE_PATH.try_into()?)
            .publish_subscribe()
            .open_or_create()?;
        let publisher = service.publisher_builder().create()?;
        Ok(Self {
            publisher
        })
    }
}
