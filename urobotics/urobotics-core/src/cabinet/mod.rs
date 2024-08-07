use std::{
    fs::File,
    io::{BufRead, BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    sync::mpsc::{Receiver, Sender},
    time::{Duration, Instant},
};

use bincode::deserialize_from;
use chrono::{Datelike, Local, Timelike};
use fxhash::FxHashSet;
use spin_sleep::SpinSleeper;
use tasker::{
    attach_drop_guard, callbacks::caller::try_drop_this_callback, detach_drop_guard, task::SyncTask,
};

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

    pub fn set_cabinet_path_with_name(
        &mut self,
        root_path: impl Into<PathBuf>,
        crate_name: &str,
    ) -> &mut Self {
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

pub type DataDumpBoxed<T> = DataDump<T, Box<dyn FnMut(T)>>;

impl<T: Send + 'static, F: FnMut(T) -> std::io::Result<()> + Send + 'static> SyncTask
    for DataDump<T, F>
{
    type Output = std::io::Result<()>;

    fn run(mut self) -> std::io::Result<()> {
        attach_drop_guard();
        drop(self.sender);
        loop {
            let Ok(data) = self.receiver.recv() else {
                break;
            };
            (self.writer)(data)?;
        }
        detach_drop_guard();
        Ok(())
    }
}

impl<T, F> DataDump<T, F> {
    pub fn new_with_func(f: F) -> Self {
        let (sender, receiver) = std::sync::mpsc::channel();
        Self {
            receiver,
            sender,
            writer: f,
        }
    }

    pub fn boxed_writer(self) -> DataDump<T, Box<dyn FnMut(T) -> std::io::Result<()>>>
    where
        F: FnMut(T) -> std::io::Result<()> + 'static,
    {
        DataDump {
            receiver: self.receiver,
            sender: self.sender,
            writer: Box::new(self.writer),
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

    pub fn new_with_bincode_writer(
        mut writer: impl Write,
    ) -> DataDump<T, impl FnMut(T) -> std::io::Result<()>>
    where
        T: serde::Serialize,
    {
        DataDump::new_with_func(
            move |data| match bincode::serialize_into(&mut writer, &data) {
                Ok(()) => Ok(()),
                Err(e) => match *e {
                    bincode::ErrorKind::Io(e) => Err(e),
                    _ => Err(std::io::Error::new(std::io::ErrorKind::Other, e)),
                },
            },
        )
    }

    pub fn new_with_bincode_file(
        path: impl AsRef<Path>,
    ) -> std::io::Result<DataDump<T, impl FnMut(T) -> std::io::Result<()>>>
    where
        T: serde::Serialize,
    {
        let file = File::create(path)?;
        Ok(Self::new_with_bincode_writer(BufWriter::new(file)))
    }

    pub fn new_with_text_writer(
        mut to_string: impl FnMut(T) -> String,
        mut writer: impl Write,
    ) -> DataDump<T, impl FnMut(T) -> std::io::Result<()>> {
        DataDump::new_with_func(move |data| writer.write_all(to_string(data).as_bytes()))
    }

    pub fn new_with_text_file(
        to_string: impl FnMut(T) -> String,
        path: impl AsRef<Path>,
    ) -> std::io::Result<DataDump<T, impl FnMut(T) -> std::io::Result<()>>> {
        let file = File::create(path)?;
        Ok(Self::new_with_text_writer(to_string, BufWriter::new(file)))
    }
}

pub struct DataReader<F> {
    reader: F,
}

impl<F> DataReader<F> {
    pub fn new_with_func<T>(
        mut reader: impl FnMut() -> Option<std::io::Result<T>>,
        mut callback: impl FnMut(T),
    ) -> DataReader<impl FnMut() -> Option<std::io::Result<()>>> {
        DataReader {
            reader: move || match reader()? {
                Ok(data) => {
                    callback(data);
                    Some(Ok(()))
                }
                Err(e) => Some(Err(e)),
            },
        }
    }

    pub fn new_with_bincode_reader<T>(
        mut reader: impl BufRead,
        mut callback: impl FnMut(T),
    ) -> DataReader<impl FnMut() -> Option<std::io::Result<()>>>
    where
        T: serde::de::DeserializeOwned,
    {
        DataReader {
            reader: move || {
                match reader.has_data_left() {
                    Ok(true) => {}
                    Ok(false) => {
                        return None;
                    }
                    Err(e) => {
                        return Some(Err(e));
                    }
                }
                match deserialize_from(&mut reader) {
                    Ok(data) => {
                        callback(data);
                        Some(Ok(()))
                    }
                    Err(e) => match *e {
                        bincode::ErrorKind::Io(e) => Some(Err(e)),
                        _ => Some(Err(std::io::Error::new(std::io::ErrorKind::Other, e))),
                    },
                }
            },
        }
    }

    pub fn new_with_bincode_file<T>(
        path: impl AsRef<Path>,
        callback: impl FnMut(T),
    ) -> std::io::Result<DataReader<impl FnMut() -> Option<std::io::Result<()>>>>
    where
        T: serde::de::DeserializeOwned,
    {
        let file = File::open(path)?;
        Ok(Self::new_with_bincode_reader(
            BufReader::new(file),
            callback,
        ))
    }

    pub fn new_with_text_reader<T>(
        delimit: impl Into<String>,
        mut from_string: impl FnMut(&str) -> T,
        mut callback: impl FnMut(T),
        mut reader: impl Read,
    ) -> DataReader<impl FnMut() -> Option<std::io::Result<()>>> {
        let delimit = delimit.into();
        let mut string_buffer = String::new();
        let mut bin_buffer: Vec<u8> = Vec::new();
        let mut tmp_buf = [0u8; 4096];
        DataReader {
            reader: move || loop {
                let n = match reader.read(&mut tmp_buf) {
                    Ok(n) => n,
                    Err(e) => return Some(Err(e)),
                };
                bin_buffer.extend_from_slice(&tmp_buf[..n]);
                match std::str::from_utf8(&bin_buffer) {
                    Ok(s) => {
                        string_buffer.push_str(s);
                        bin_buffer.clear();
                        if let Some(pos) = string_buffer.find(&delimit) {
                            let data = from_string(&string_buffer[..pos]);
                            callback(data);
                            string_buffer.drain(..pos + delimit.len());
                        }
                    }
                    Err(_) => {}
                }
                if n == 0 {
                    return None;
                }
            },
        }
    }

    pub fn new_with_text_file<T>(
        delimit: impl Into<String>,
        from_string: impl FnMut(&str) -> T,
        callback: impl FnMut(T),
        path: impl AsRef<Path>,
    ) -> std::io::Result<DataReader<impl FnMut() -> Option<std::io::Result<()>>>> {
        let file = File::create(path)?;
        Ok(Self::new_with_text_reader(
            delimit,
            from_string,
            callback,
            file,
        ))
    }
}

impl<F: FnMut() -> Option<std::io::Result<()>> + Send + 'static> SyncTask for DataReader<F> {
    type Output = std::io::Result<()>;

    fn run(mut self) -> Self::Output {
        loop {
            if let Some(result) = (self.reader)() {
                result?;
            } else {
                break Ok(());
            }
        }
    }
}

pub struct Recorder<T, F> {
    dump: DataDump<(Duration, T), F>,
    instant: Instant,
}

impl<T, F> Recorder<T, F> {
    pub fn new_with_dump(dump: DataDump<(Duration, T), F>) -> Self {
        Self {
            dump,
            instant: Instant::now(),
        }
    }

    pub fn get_write_callback(&self) -> impl Fn(T) {
        let instant = self.instant;
        let inner = self.dump.get_write_callback();
        move |data| {
            inner((instant.elapsed(), data));
        }
    }
}

impl<T: Send + 'static, F: FnMut((Duration, T)) -> std::io::Result<()> + Send + 'static> SyncTask
    for Recorder<T, F>
{
    type Output = std::io::Result<()>;

    fn run(self) -> std::io::Result<()> {
        self.dump.run()
    }
}

pub struct Playback<F> {
    reader: DataReader<F>,
}

impl<F> Playback<F> {
    pub fn new_with_bincode_reader<T>(
        mut reader: impl BufRead,
        mut callback: impl FnMut(T),
    ) -> Playback<impl FnMut() -> Option<std::io::Result<()>>>
    where
        T: serde::de::DeserializeOwned,
    {
        let instant = Instant::now();
        let sleeper = SpinSleeper::default();
        Playback {
            reader: DataReader {
                reader: move || {
                    match reader.has_data_left() {
                        Ok(true) => {}
                        Ok(false) => {
                            return None;
                        }
                        Err(e) => {
                            return Some(Err(e));
                        }
                    }
                    match deserialize_from::<_, (Duration, T)>(&mut reader) {
                        Ok((duration, data)) => {
                            sleeper.sleep(duration - instant.elapsed());
                            callback(data);
                            Some(Ok(()))
                        }
                        Err(e) => match *e {
                            bincode::ErrorKind::Io(e) => Some(Err(e)),
                            _ => Some(Err(std::io::Error::new(std::io::ErrorKind::Other, e))),
                        },
                    }
                },
            },
        }
    }

    pub fn new_with_bincode_file<T>(
        path: impl AsRef<Path>,
        callback: impl FnMut(T),
    ) -> std::io::Result<Playback<impl FnMut() -> Option<std::io::Result<()>>>>
    where
        T: serde::de::DeserializeOwned,
    {
        let file = File::open(path)?;
        Ok(Self::new_with_bincode_reader(
            BufReader::new(file),
            callback,
        ))
    }

    pub fn new_with_text_reader<T>(
        delimit: impl Into<String>,
        mut from_string: impl FnMut(&str) -> (Duration, T),
        mut callback: impl FnMut(T),
        mut reader: impl Read,
    ) -> Playback<impl FnMut() -> Option<std::io::Result<()>>> {
        let delimit = delimit.into();
        let mut string_buffer = String::new();
        let mut bin_buffer: Vec<u8> = Vec::new();
        let mut tmp_buf = [0u8; 4096];
        let instant = Instant::now();
        let sleeper = SpinSleeper::default();
        Playback {
            reader: DataReader {
                reader: move || loop {
                    let n = match reader.read(&mut tmp_buf) {
                        Ok(n) => n,
                        Err(e) => return Some(Err(e)),
                    };
                    bin_buffer.extend_from_slice(&tmp_buf[..n]);
                    match std::str::from_utf8(&bin_buffer) {
                        Ok(s) => {
                            string_buffer.push_str(s);
                            bin_buffer.clear();
                            if let Some(pos) = string_buffer.find(&delimit) {
                                let (duration, data) = from_string(&string_buffer[..pos]);
                                sleeper.sleep(duration - instant.elapsed());
                                callback(data);
                                string_buffer.drain(..pos + delimit.len());
                            }
                        }
                        Err(_) => {}
                    }
                    if n == 0 {
                        return None;
                    }
                },
            },
        }
    }

    pub fn new_with_text_file<T>(
        delimit: impl Into<String>,
        from_string: impl FnMut(&str) -> (Duration, T),
        callback: impl FnMut(T),
        path: impl AsRef<Path>,
    ) -> std::io::Result<Playback<impl FnMut() -> Option<std::io::Result<()>>>> {
        let file = File::create(path)?;
        Ok(Self::new_with_text_reader(
            delimit,
            from_string,
            callback,
            file,
        ))
    }
}

impl<F: FnMut() -> Option<std::io::Result<()>> + Send + 'static> SyncTask for Playback<F> {
    type Output = std::io::Result<()>;

    fn run(self) -> std::io::Result<()> {
        self.reader.run()
    }
}
