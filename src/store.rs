use crate::chain::{extend, GENESIS};
use crate::record::{canonical, Record, RecordError};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const SEGMENT: &str = "segment-0.log";
const LEN_BYTES: usize = 4;
const DIGEST_BYTES: usize = 32;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("could not open the log at {path}: {source}")]
    Open {
        path: String,
        source: std::io::Error,
    },
    #[error("could not write to the log: {0}")]
    Write(std::io::Error),
    #[error("could not flush the log to disk: {0}")]
    Sync(std::io::Error),
    #[error(
        "the record at byte {at} is complete but its trailing digest does not match its bytes; \
         this is an alteration, not a torn write, and recovery will not skip it"
    )]
    Corrupt { at: u64 },
    #[error("the record at byte {at} could not be parsed: {source}")]
    Parse { at: u64, source: serde_json::Error },
    #[error("a record declares a length of {len} bytes, beyond any plausible record")]
    LengthImplausible { len: u32 },
    #[error(transparent)]
    Record(#[from] RecordError),
}

/// A record longer than this is refused rather than allocated. The bytes come
/// off disk and a corrupted length header would otherwise be a memory
/// exhaustion.
const MAX_RECORD: u32 = 1 << 20;

pub struct Store {
    file: File,
    path: PathBuf,
    records: Vec<Record>,
    head: [u8; 32],
}

impl Store {
    /// Open the log at `dir`, recovering whatever is intact.
    ///
    /// A trailing partial record is truncated: a process died mid-write and
    /// that record was never acknowledged, so no caller believes it exists.
    /// A *complete* record whose digest does not match is a different thing
    /// entirely and is an error — see `StoreError::Corrupt`.
    pub fn open(dir: &Path) -> Result<Self, StoreError> {
        std::fs::create_dir_all(dir).map_err(|source| StoreError::Open {
            path: dir.display().to_string(),
            source,
        })?;
        let path = dir.join(SEGMENT);
        let mut file = OpenOptions::new()
            .read(true)
            .append(true)
            .create(true)
            .open(&path)
            .map_err(|source| StoreError::Open {
                path: path.display().to_string(),
                source,
            })?;

        let mut bytes = Vec::new();
        file.seek(SeekFrom::Start(0)).map_err(StoreError::Write)?;
        file.read_to_end(&mut bytes).map_err(StoreError::Write)?;

        let mut records = Vec::new();
        let mut head = GENESIS;
        let mut at: u64 = 0;
        let mut cursor: usize = 0;

        loop {
            if cursor + LEN_BYTES > bytes.len() {
                break;
            }
            let len_arr: [u8; LEN_BYTES] = match bytes[cursor..cursor + LEN_BYTES].try_into() {
                Ok(a) => a,
                Err(_) => break,
            };
            let len = u32::from_be_bytes(len_arr);
            if len > MAX_RECORD {
                return Err(StoreError::LengthImplausible { len });
            }
            let body_start = cursor + LEN_BYTES;
            let body_end = body_start + len as usize;
            let frame_end = body_end + DIGEST_BYTES;
            if frame_end > bytes.len() {
                // Torn write. Everything from `at` onward was never acked.
                break;
            }
            let body = &bytes[body_start..body_end];
            let want: [u8; DIGEST_BYTES] = Sha256::digest(body).into();
            let got: [u8; DIGEST_BYTES] = match bytes[body_end..frame_end].try_into() {
                Ok(a) => a,
                Err(_) => return Err(StoreError::Corrupt { at }),
            };
            if want != got {
                return Err(StoreError::Corrupt { at });
            }
            let r: Record =
                serde_json::from_slice(body).map_err(|source| StoreError::Parse { at, source })?;
            head = extend(head, &r)?;
            records.push(r);
            cursor = frame_end;
            at = cursor as u64;
        }

        if (cursor as u64) < bytes.len() as u64 {
            file.set_len(cursor as u64).map_err(StoreError::Write)?;
        }
        file.seek(SeekFrom::End(0)).map_err(StoreError::Write)?;

        Ok(Self {
            file,
            path,
            records,
            head,
        })
    }

    pub fn records(&self) -> &[Record] {
        &self.records
    }

    pub fn head(&self) -> [u8; 32] {
        self.head
    }

    pub fn next_step(&self) -> u64 {
        self.records.len() as u64
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Write a batch and `fsync` **once** for the whole batch.
    ///
    /// The caller must not acknowledge any member of the batch until this
    /// returns `Ok`. That is C7.1.3, and Task 7 is where the acknowledgement
    /// path is built so it cannot be got wrong.
    ///
    /// On any error the in-memory state is left untouched, so a failed batch
    /// cannot advance `head` or `next_step` and be acked by a later caller.
    pub fn append_batch(&mut self, records: &[Record]) -> Result<(), StoreError> {
        let mut buf = Vec::new();
        let mut head = self.head;
        for r in records {
            let body = canonical(r)?.into_bytes();
            let len = u32::try_from(body.len())
                .map_err(|_| StoreError::LengthImplausible { len: u32::MAX })?;
            if len > MAX_RECORD {
                return Err(StoreError::LengthImplausible { len });
            }
            let d: [u8; DIGEST_BYTES] = Sha256::digest(&body).into();
            buf.extend_from_slice(&len.to_be_bytes());
            buf.extend_from_slice(&body);
            buf.extend_from_slice(&d);
            head = extend(head, r)?;
        }

        self.file.write_all(&buf).map_err(StoreError::Write)?;
        self.file.sync_data().map_err(StoreError::Sync)?;

        self.records.extend_from_slice(records);
        self.head = head;
        Ok(())
    }
}
