//! Append each record to a local file, one JSON document per line.

use super::{record_file_options, required, Sink, SinkError};
use crate::config::SubscriberConfig;
use crate::error::Problem;
use async_trait::async_trait;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};

pub struct LocalLogSink {
    path: PathBuf,
    file: Mutex<Option<File>>,
}

impl LocalLogSink {
    pub fn from_config(config: &SubscriberConfig) -> Result<Self, Problem> {
        Ok(LocalLogSink {
            path: PathBuf::from(required(&config.local_logfile, "local_logfile")?),
            file: Mutex::new(None),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Option<File>> {
        self.file.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn open(&self) -> std::io::Result<File> {
        record_file_options().append(true).open(&self.path)
    }
}

#[async_trait]
impl Sink for LocalLogSink {
    /// Create the file, so a path that cannot be written is reported before
    /// the first record.
    async fn prepare(&self) -> Result<(), SinkError> {
        let mut file = self.lock();
        if file.is_none() {
            *file = Some(self.open()?);
        }
        Ok(())
    }

    async fn send(&self, _topic: &str, record: &str) -> Result<(), SinkError> {
        let mut line = Vec::with_capacity(record.len() + 1);
        line.extend_from_slice(record.trim().as_bytes());
        line.push(b'\n');
        let mut guard = self.lock();
        if guard.is_none() {
            *guard = Some(self.open()?);
        }
        // One write per record keeps lines whole when two subscribers share
        // a file.
        let written = match guard.as_mut() {
            Some(file) => file.write_all(&line),
            None => Ok(()),
        };
        if let Err(e) = written {
            // Reopen on the next record: the file may have been rotated away.
            *guard = None;
            return Err(e.into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{subscriber, temp_path};

    #[tokio::test]
    async fn records_are_appended_one_per_line() {
        let path = temp_path("local-log.log");
        let sink = LocalLogSink::from_config(&subscriber(&format!(
            "    subscriber_type: local\n    subtype: locallog\n    local_logfile: \"{}\"\n",
            path.display()
        )))
        .unwrap();
        sink.prepare().await.unwrap();
        assert!(path.exists());
        sink.send("WX/Heartbeat", r#"{"n":1}"#).await.unwrap();
        sink.send("WX/Heartbeat", " {\"n\":2}\n").await.unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{\"n\":1}\n{\"n\":2}\n"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_file_is_created_without_access_for_others() {
        use std::os::unix::fs::PermissionsExt;
        let path = temp_path("local-log-mode.log");
        let sink = LocalLogSink::from_config(&subscriber(&format!(
            "    subscriber_type: local\n    local_logfile: \"{}\"\n",
            path.display()
        )))
        .unwrap();
        sink.prepare().await.unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o007, 0, "mode {mode:o}");
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn a_path_that_cannot_be_written_fails_prepare() {
        let sink = LocalLogSink::from_config(&subscriber(
            "    subscriber_type: local\n    local_logfile: /nonexistent-directory/records.log\n",
        ))
        .unwrap();
        assert!(matches!(sink.prepare().await, Err(SinkError::Io(_))));
    }
}
