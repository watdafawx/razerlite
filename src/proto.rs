//! Razer 90-byte feature-report protocol.
//!
//! Layout (same as Synapse 4 and OpenRazer):
//!   [0] status   [1] transaction id   [2..4] remaining packets (u16 BE)
//!   [4] protocol type   [5] data size   [6] command class   [7] command id
//!   [8..88] arguments   [88] xor checksum of bytes 2..88   [89] reserved

use anyhow::{Context, Result};
use hidapi::HidDevice;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

pub const REPORT_LEN: usize = 90;

#[derive(Debug, Clone, Copy)]
pub struct Cmd {
    pub size: u8,
    pub class: u8,
    pub id: u8,
}

pub const fn cmd(size: u8, class: u8, id: u8) -> Cmd {
    Cmd { size, class, id }
}

static TXN: AtomicU8 = AtomicU8::new(0);

fn next_txn() -> u8 {
    // Synapse cycles 0..=31.
    TXN.fetch_add(1, Ordering::Relaxed) % 32
}

pub fn build(c: Cmd, args: &[u8], size_override: Option<u8>) -> [u8; REPORT_LEN] {
    let mut r = [0u8; REPORT_LEN];
    r[1] = next_txn();
    r[5] = size_override.unwrap_or(c.size);
    r[6] = c.class;
    r[7] = c.id;
    let n = args.len().min(80);
    r[8..8 + n].copy_from_slice(&args[..n]);
    r[88] = r[2..88].iter().fold(0, |a, b| a ^ b);
    r
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    New,
    Busy,
    Ok,
    Fail,
    Timeout,
    NotSupported,
    Unknown(u8),
}

impl From<u8> for Status {
    fn from(v: u8) -> Self {
        match v {
            0 => Status::New,
            1 => Status::Busy,
            2 => Status::Ok,
            3 => Status::Fail,
            4 => Status::Timeout,
            5 => Status::NotSupported,
            x => Status::Unknown(x),
        }
    }
}

pub struct Response {
    pub raw: [u8; REPORT_LEN],
}

impl Response {
    pub fn status(&self) -> Status {
        self.raw[0].into()
    }
    pub fn args(&self) -> &[u8] {
        &self.raw[8..88]
    }
}

/// Send one command and read the reply.
/// Retries while the device is busy, and resends if another program (e.g. Synapse)
/// grabbed the shared reply buffer first.
pub fn transact(dev: &HidDevice, c: Cmd, args: &[u8], size_override: Option<u8>) -> Result<Response> {
    let mut last_err = None;
    for attempt in 0..8u64 {
        match transact_once(dev, c, args, size_override) {
            Ok(r) => return Ok(r),
            Err(Attempt::Retry(e)) => {
                last_err = Some(e);
                // Vary the timing so we land between another program's packets.
                std::thread::sleep(Duration::from_millis(1 + (attempt * 7) % 13));
            }
            Err(Attempt::Fatal(e)) => return Err(e),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no reply")))
}

enum Attempt {
    Retry(anyhow::Error),
    Fatal(anyhow::Error),
}

fn transact_once(dev: &HidDevice, c: Cmd, args: &[u8], size_override: Option<u8>) -> Result<Response, Attempt> {
    let req = build(c, args, size_override);
    let mut out = [0u8; REPORT_LEN + 1]; // report id 0 prefix
    out[1..].copy_from_slice(&req);
    dev.send_feature_report(&out)
        .context("send_feature_report")
        .map_err(Attempt::Fatal)?;
    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(3));
        let mut inb = [0u8; REPORT_LEN + 1];
        let n = dev
            .get_feature_report(&mut inb)
            .context("get_feature_report")
            .map_err(Attempt::Fatal)?;
        if n < REPORT_LEN {
            return Err(Attempt::Fatal(anyhow::anyhow!("short reply ({n} bytes)")));
        }
        // hidapi on Windows keeps the report id byte at [0].
        let mut raw = [0u8; REPORT_LEN];
        raw.copy_from_slice(&inb[n - REPORT_LEN..n]);
        let resp = Response { raw };
        let ours = raw[1] == req[1] && raw[6] == c.class && raw[7] == c.id;
        match resp.status() {
            Status::Busy | Status::New => continue,
            _ if !ours => {
                return Err(Attempt::Retry(anyhow::anyhow!(
                    "reply for other command {:02x}:{:02x} (is Synapse busy with the device?)",
                    raw[6],
                    raw[7]
                )));
            }
            Status::Ok => return Ok(resp),
            s => {
                return Err(Attempt::Fatal(anyhow::anyhow!(
                    "device returned {s:?} for {:02x}:{:02x}",
                    c.class,
                    c.id
                )));
            }
        }
    }
    Err(Attempt::Retry(anyhow::anyhow!("device stayed busy")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_layout_and_checksum() {
        let r = build(cmd(80, 0x02, 0x12), &[1, 18, 0, 142, 113], None);
        assert_eq!(r.len(), REPORT_LEN);
        assert_eq!((r[5], r[6], r[7]), (80, 0x02, 0x12));
        assert_eq!(&r[8..13], &[1, 18, 0, 142, 113]);
        let xor = r[2..88].iter().fold(0u8, |a, b| a ^ b);
        assert_eq!(r[88], xor);
        assert!(r[1] < 32, "transaction id cycles 0..=31");
    }

    #[test]
    fn size_override_and_arg_cap() {
        let r = build(cmd(80, 0x0f, 0x02), &[0xaa; 100], Some(12));
        assert_eq!(r[5], 12);
        assert!(r[8..88].iter().all(|&b| b == 0xaa));
        assert_eq!(r[89], 0);
    }
}
