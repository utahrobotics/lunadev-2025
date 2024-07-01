use std::path::PathBuf;

use chrono::{Datelike, Local, Timelike};

pub struct CabinetBuilder {
    pub path: PathBuf,
    pub files_to_copy: Vec<PathBuf>,
}


impl CabinetBuilder {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            files_to_copy: Vec::new(),
        }
    }

    pub fn new_with_crate_name(root_path: impl Into<PathBuf>, crate_name: &str) -> Self {
        let mut path = root_path.into();
        path.push(crate_name);
        let datetime = Local::now();
        let log_folder_name = format!(
            "{}-{:0>2}-{:0>2}={:0>2}-{:0>2}-{:0>2}",
            datetime.year(),
            datetime.month(),
            datetime.day(),
            datetime.hour(),
            datetime.minute(),
            datetime.second(),
        );
        path.push(log_folder_name);
        
        Self::new(path)
    }

    pub fn build(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.path)?;
        for file in &self.files_to_copy {
            let file_name = file.file_name().unwrap();
            let mut new_file_path = self.path.clone();
            new_file_path.push(file_name);
            std::fs::copy(file, new_file_path)?;
        }
        std::env::set_current_dir(&self.path)
    }
}


#[macro_export]
macro_rules! default_cabinet_builder {
    () => {
        CabinetBuilder::new_with_crate_name("dump", env!("CARGO_PKG_NAME"))
    };
}
