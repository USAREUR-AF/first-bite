//! Append each record to a local file: one JSON document per line, or with
//! `encoding: cbor` one CBOR item after another (a CBOR sequence, RFC 8742).

use super::{record_file_options, required, Sink, SinkError};
use crate::config::SubscriberConfig;
use crate::encoding::{json_to_cbor, Encoding};
use crate::error::Problem;
use async_trait::async_trait;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};

pub struct LocalLogSink {
    path: PathBuf,
    encoding: Encoding,
    file: Mutex<Option<File>>,
}

impl LocalLogSink {
    pub fn from_config(config: &SubscriberConfig) -> Result<Self, Problem> {
        Ok(LocalLogSink {
            path: PathBuf::from(required(&config.local_logfile, "local_logfile")?),
            encoding: Encoding::from_config(config.encoding.as_deref())?,
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
        let line = match self.encoding {
            Encoding::Json => {
                let mut line = Vec::with_capacity(record.len() + 1);
                line.extend_from_slice(record.trim().as_bytes());
                line.push(b'\n');
                line
            }
            // Each item says where it ends, so nothing separates them.
            Encoding::Cbor => json_to_cbor(record)?,
        };
        let mut guard = self.lock();
        if guard.is_none() {
            *guard = Some(self.open()?);
        }
        // One write per record keeps records whole when two subscribers
        // share a file.
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
    async fn cbor_records_are_appended_as_a_sequence() {
        let path = temp_path("local-log.cborseq");
        let sink = LocalLogSink::from_config(&subscriber(&format!(
            "    subscriber_type: local\n    local_logfile: \"{}\"\n    encoding: cbor\n",
            path.display()
        )))
        .unwrap();
        sink.send("WX/Heartbeat", r#"{"n":1}"#).await.unwrap();
        sink.send("WX/Heartbeat", r#"{"n":[true,null]}"#)
            .await
            .unwrap();
        let written = std::fs::read(&path).unwrap();
        assert_eq!(
            written,
            [0xa1, 0x61, b'n', 0x01, 0xa1, 0x61, b'n', 0x82, 0xf5, 0xf6]
        );
        // Read back item by item.
        let mut rest = &written[..];
        let mut items = Vec::new();
        while !rest.is_empty() {
            let item: ciborium::Value = ciborium::from_reader(&mut rest).unwrap();
            items.push(serde_json::to_value(&item).unwrap());
        }
        assert_eq!(
            items,
            [
                serde_json::json!({"n": 1}),
                serde_json::json!({"n": [true, null]})
            ]
        );
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
