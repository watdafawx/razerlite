//! Read-only: samples the keyboard's reply buffer to see what another program (Synapse)
//! is sending it. Never sends a command.
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

fn main() {
    let api = hidapi::HidApi::new().unwrap();
    let info = api.device_list().find(|d| d.vendor_id() == 0x1532 && d.product_id() == 0x0266 && d.interface_number() == 3).expect("keyboard");
    let dev = info.open_device(&api).unwrap();
    let mut seen: BTreeMap<(u8, u8), usize> = BTreeMap::new();
    let (mut changes, mut last_txn, mut samples) = (0, 255u8, 0);
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        let mut b = [0u8; 91];
        if let Ok(n) = dev.get_feature_report(&mut b) {
            let r = &b[n - 90..n];
            samples += 1;
            if r[1] != last_txn {
                changes += 1;
                last_txn = r[1];
                *seen.entry((r[6], r[7])).or_default() += 1;
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    println!("{samples} samples in 3 s, {changes} new replies (~{}/s)", changes / 3);
    for ((c, i), n) in seen {
        println!("  {c:02x}:{i:02x}  x{n}");
    }
}
