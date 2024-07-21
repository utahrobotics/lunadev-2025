use std::{fs::File, io::{BufWriter, Write}, path::{Path, PathBuf}, sync::mpsc::{Receiver, Sender}, time::{Duration, Instant}};

use chrono::{Datelike, Local, Timelike};
use fxhash::FxHashSet;
use tasker::{attach_drop_guard, callbacks::caller::try_drop_this_callback, detach_drop_guard, task::SyncTask};

pub struct CabinetBuilder {
    pub path: PathBuf,
    pub files_to_copy: Vec<PathBuf>,
    pub create_symlinks_for: Vec<PathBuf>,
}

impl CabinetBuilder {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            files_to_copy: Vec::new(),
            create_symlinks_for: Vec::new(),
        }
    }

    pub fn new_with_crate_name(root_path: impl Into<PathBuf>, crate_name: &str) -> Self {
        let mut out = Self::new(PathBuf::new());
        out.set_cabinet_path_with_name(root_path, crate_name);
        out
    }

    pub fn set_cabinet_path(&mut self, path: impl Into<PathBuf>) -> &mut Self {
        self.path = path.into();
        self
    }

    pub fn set_cabinet_path_with_name(&mut self, root_path: impl Into<PathBuf>, crate_name: &str) -> &mut Self {
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

        self.set_cabinet_path(path)
    }

    pub fn set_files_to_copy(&mut self, files: Vec<PathBuf>) -> &mut Self {
        self.files_to_copy = files;
        self
    }

    pub fn set_create_symlinks_for(&mut self, paths: Vec<PathBuf>) -> &mut Self {
        self.create_symlinks_for = paths;
        self
    }

    pub fn add_file_to_copy(&mut self, file: impl Into<PathBuf>) -> &mut Self {
        self.files_to_copy.push(file.into());
        self
    }

    pub fn create_symlink_for(&mut self, path: impl Into<PathBuf>) -> &mut Self {
        self.create_symlinks_for.push(path.into());
        self
    }

    pub fn build(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.path)?;
        for file in FxHashSet::from_iter(self.files_to_copy.iter()) {
            let file_name = file.file_name().unwrap();
            let mut new_file_path = self.path.clone();
            new_file_path.push(file_name);
            std::fs::copy(file, new_file_path)?;
        }
        for path in FxHashSet::from_iter(self.create_symlinks_for.iter()) {
            let name = path.file_name().expect("Failed to get file name");
            let path = path.canonicalize()?;
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(path, self.path.join(name))?;
            }
            #[cfg(windows)]
            {
                if path.is_dir() {
                    std::os::windows::fs::symlink_dir(path, self.path.join(name))?;
                } else {
                    std::os::windows::fs::symlink_file(path, self.path.join(name))?;
                }
            }
        }
        std::env::set_current_dir(&self.path)
    }
}

#[macro_export]
macro_rules! default_cabinet_builder {
    () => {
        CabinetBuilder::new_with_crate_name("cabinet", env!("CARGO_PKG_NAME"))
    };
}


pub struct DataDump<T, F> {
    receiver: Receiver<T>,
    sender: Sender<T>,
    writer: F,
}


impl<T: Send + 'static, F: FnMut(T) + Send + 'static> SyncTask for DataDump<T, F> {
    type Output = std::io::Result<()>;

    fn run(mut self) -> std::io::Result<()> {
        attach_drop_guard();
        drop(self.sender);
        loop {
            let Ok(data) = self.receiver.recv() else { break; };
            (self.writer)(data);
        }
        detach_drop_guard();
        Ok(())
    }
}


impl<T, F: FnMut(T)> DataDump<T, F> {
    pub fn new_with_func(f: F) -> Self {
        let (sender, receiver) = std::sync::mpsc::channel();
        Self {
            receiver,
            sender,
            writer: f,
        }
    }

    pub fn get_write_callback(&self) -> impl Fn(T) {
        let sender = self.sender.clone();
        move |data| {
            if sender.send(data).is_err() {
                try_drop_this_callback();
            }
        }
    }
    
    pub fn new_with_bincode_writer(mut writer: impl Write) -> DataDump<T, impl FnMut(T)>
        where 
            T: serde::Serialize,
    {
        DataDump::new_with_func(
            move |data| bincode::serialize_into(&mut writer, &data).expect("Failed to serialize into writer"),
        )
    }

    pub fn new_from_bincode_file(path: impl AsRef<Path>) -> std::io::Result<DataDump<T, impl FnMut(T)>>
    where 
        T: serde::Serialize,
    {
        let file = File::create(path)?;
        Ok(Self::new_with_bincode_writer(BufWriter::new(file)))
    }
    
    pub fn new_with_text_writer(mut to_string: impl FnMut(T) -> String, mut writer: impl Write) -> DataDump<T, impl FnMut(T)> {
        DataDump::new_with_func(
            move |data| writer.write_all(to_string(data).as_bytes()).expect("Failed to serialize into writer"),
        )
    }

    pub fn new_from_text_file(to_string: impl FnMut(T) -> String, path: impl AsRef<Path>) -> std::io::Result<DataDump<T, impl FnMut(T)>>
    {
        let file = File::create(path)?;
        Ok(Self::new_with_text_writer(to_string, BufWriter::new(file)))
    }
}


pub struct Recorder<T, F> {
    dump: DataDump<(Duration, T), F>,
}


impl<T, F: FnMut((Duration, T))> Recorder<T, F> {
    pub fn new_with_dump(dump: DataDump<(Duration, T), F>) -> Self {
        Self {
            dump
        }
    }

    pub fn get_write_callback(&self, instant: Instant) -> impl Fn(T) {
        let inner = self.dump.get_write_callback();
        move |data| {
            inner((instant.elapsed(), data));
        }
    }
}


impl<T: Send + 'static, F: FnMut((Duration, T)) + Send + 'static> SyncTask for Recorder<T, F> {
    type Output = std::io::Result<()>;

    fn run(self) -> std::io::Result<()> {
        self.dump.run()
    }
}