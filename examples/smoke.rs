use std::collections::{BTreeMap, HashMap};

use ekv::config::{MAX_PAGE_COUNT, PAGE_SIZE};
use ekv::flash::MemFlash;
use ekv::{Config, Database};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use rand::Rng;

const KEY_COUNT: usize = 1000;
const KEY_MIN_LEN: usize = 1;
const KEY_MAX_LEN: usize = 10;
const VAL_MIN_LEN: usize = 1;
const VAL_MAX_LEN: usize = 10;
const TX_MIN_COUNT: usize = 1;
const TX_MAX_COUNT: usize = 100;

fn rand(max: usize) -> usize {
    rand::thread_rng().gen_range(0..max)
}

fn rand_between(from: usize, to: usize) -> usize {
    rand::thread_rng().gen_range(from..=to)
}

fn rand_data(len: usize) -> Vec<u8> {
    let mut res = vec![0; len];
    rand::thread_rng().fill(&mut res[..]);
    res
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    env_logger::init();

    println!("Hi there!");
    println!("flash size: {}", PAGE_SIZE * MAX_PAGE_COUNT);
    println!(
        "avg data size: {}",
        (KEY_COUNT + TX_MAX_COUNT) * (KEY_MIN_LEN + KEY_MAX_LEN + VAL_MIN_LEN + VAL_MAX_LEN) / 2
    );
    println!(
        "max data size: {}",
        (KEY_COUNT + TX_MAX_COUNT) * (KEY_MAX_LEN + VAL_MAX_LEN)
    );

    // Generate keys
    let mut keys = Vec::new();
    keys.push(b"foo".to_vec());
    while keys.len() < KEY_COUNT {
        let key = rand_data(rand_between(KEY_MIN_LEN, KEY_MAX_LEN));
        if !keys.contains(&key) {
            keys.push(key)
        }
    }

    let mut f = MemFlash::new();
    let config = Config::default();
    let db = Database::<_, NoopRawMutex>::new(&mut f, config);
    db.format().await.unwrap();

    // Mirror hashmap. Should always match F
    let mut m = HashMap::new();

    let mut buf = [0; VAL_MAX_LEN];

    for _ in 0..10000 {
        let tx_count = rand_between(TX_MIN_COUNT, TX_MAX_COUNT);
        let mut tx = BTreeMap::new();

        for _ in 0..tx_count {
            let key = &keys[rand(KEY_COUNT)];
            let value = rand_data(rand_between(VAL_MIN_LEN, VAL_MAX_LEN));
            tx.insert(key, value);
        }

        // Write to DB
        let mut wtx = db.write_transaction().await;
        log::debug!("start tx");
        for (key, value) in &tx {
            log::debug!("write {:02x?} = {:02x?}", key, value);
            wtx.write(key, value).await.unwrap();
        }
        wtx.commit().await.unwrap();

        // Write to mirror
        for (&key, value) in &tx {
            m.insert(key.clone(), value.clone());
        }

        // Check everything
        for key in &keys {
            log::debug!("read {:02x?}", key);
            let mut rtx = db.read_transaction().await;
            let n = rtx.read(key, &mut buf).await.unwrap();
            let got_val = &buf[..n];
            let val = m.get(key).map(|v| &v[..]).unwrap_or(&[]);

            if val != got_val {
                panic!(
                    "mismatch found!\nkey={:02x?}\nwant val={:02x?}\ngot val={:02x?}",
                    key, val, got_val
                );
            }
        }
    }

    // remount, recheck everything.
    let config = Config::default();
    let db = Database::<_, NoopRawMutex>::new(&mut f, config);

    for key in &keys {
        let mut rtx = db.read_transaction().await;
        let n = rtx.read(key, &mut buf).await.unwrap();
        let got_val = &buf[..n];
        let val = m.get(key).map(|v| &v[..]).unwrap_or(&[]);

        if val != got_val {
            panic!(
                "mismatch found!\nkey={:02x?}\nwant val={:02x?}\ngot val={:02x?}",
                key, val, got_val
            );
        }
    }

    std::fs::write("out.bin", &f.data).unwrap();
}
