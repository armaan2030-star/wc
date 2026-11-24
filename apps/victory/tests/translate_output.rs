use std::fs;
use std::process::Command;

use victory_shader::{Container, FourCC};

/// One compute stage with a 64x1x1 group and a single s_endpgm instruction.
fn synthetic_compute(include_ls: bool) -> Vec<u8> {
    const RECORD: usize = 0x68;
    const STAGE: usize = RECORD + 0x54;
    const CODE: usize = STAGE + 0x34;
    let mut payload = vec![0; CODE];
    payload[..8].copy_from_slice(&6u64.to_le_bytes());
    for slot in [0x08, 0x10, 0x18, 0x20, 0x28, 0x30, 0x48] {
        payload[slot..slot + 8].copy_from_slice(&0xDEAD_BEEF_DEAD_BEEFu64.to_le_bytes());
    }
    if include_ls {
        payload[0x08..0x10].copy_from_slice(&(RECORD as u64).to_le_bytes());
    }
    payload[0x60..0x68].copy_from_slice(&(RECORD as u64).to_le_bytes());
    payload[RECORD..RECORD + 8].copy_from_slice(&(CODE as u64).to_le_bytes());
    payload[RECORD + 8..RECORD + 16].copy_from_slice(&4u64.to_le_bytes());
    for (axis, threads) in [64u32, 1, 1].into_iter().enumerate() {
        let offset = STAGE + 0x28 + axis * 4;
        payload[offset..offset + 4].copy_from_slice(&threads.to_le_bytes());
    }
    payload.extend_from_slice(&0xBF81_0000u32.to_le_bytes());

    let mut fssm = b"FSSM".to_vec();
    fssm.extend_from_slice(&0x101u32.to_le_bytes());
    fssm.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    fssm.extend_from_slice(&payload);
    let stream = miniz_oxide::deflate::compress_to_vec_zlib(&fssm, 6);
    let mut xbch = b"XBCH".to_vec();
    xbch.extend_from_slice(&3u32.to_le_bytes());
    xbch.extend_from_slice(&(stream.len() as u64).to_le_bytes());
    xbch.extend_from_slice(&(fssm.len() as u64).to_le_bytes());
    xbch.extend_from_slice(&stream);

    let mut dxil = vec![0; 0x18];
    dxil.extend_from_slice(b"XDAT");
    for field in [1u32, 0x24, 0x0D, 0x30, xbch.len() as u32] {
        dxil.extend_from_slice(&field.to_le_bytes());
    }
    dxil.extend_from_slice(&xbch);

    let mut dxbc = vec![0; 0x24];
    dxbc[..4].copy_from_slice(b"DXBC");
    dxbc[0x14..0x16].copy_from_slice(&1u16.to_le_bytes());
    dxbc[0x18..0x1C].copy_from_slice(&(0x2C + dxil.len() as u32).to_le_bytes());
    dxbc[0x1C..0x20].copy_from_slice(&1u32.to_le_bytes());
    dxbc[0x20..0x24].copy_from_slice(&0x24u32.to_le_bytes());
    dxbc.extend_from_slice(b"DXIL");
    dxbc.extend_from_slice(&(dxil.len() as u32).to_le_bytes());
    dxbc.extend_from_slice(&dxil);
    dxbc
}

#[test]
fn export_continues_after_failures_and_preserves_existing_dxil() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path();
    let output = input.join("export");
    fs::write(input.join("bad.dxbc"), b"invalid container").unwrap();
    let mixed = synthetic_compute(true);
    fs::write(input.join("mixed.dxbc"), &mixed).unwrap();
    // These containers have byte-identical DXIL parts and differ outside them.
    let mut variant = mixed;
    variant[4] = 1;
    fs::write(input.join("second.dxbc"), variant).unwrap();

    let run = || {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_victory"));
        cmd.arg("translate")
            .arg(input)
            .arg("--output")
            .arg(&output)
            .args(["--timings", "--jobs", "2"]);
        if let Some(validator) = std::env::var_os("VICTORY_TEST_DXIL") {
            cmd.arg("--validate").env("VICTORY_DXIL", validator);
        }
        cmd.output().unwrap()
    };
    let first = run();
    assert!(!first.status.success());
    let stdout = String::from_utf8_lossy(&first.stdout);
    assert!(stdout.contains("DXIL files   2 written"), "{stdout}");
    assert!(
        stdout.contains("unsupported hardware shader stage"),
        "{stdout}"
    );
    assert!(stdout.contains("no shader containers found"), "{stdout}");
    assert!(stdout.contains("translation"), "{stdout}");

    let files: Vec<_> = fs::read_dir(&output)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            let bytes = fs::read(&path).unwrap();
            assert!(
                Container::parse(&bytes)
                    .unwrap()
                    .part(FourCC::DXIL)
                    .is_some()
            );
            (path, bytes)
        })
        .collect();
    assert_eq!(files.len(), 2);

    let repeated = run();
    assert!(!repeated.status.success());
    let stdout = String::from_utf8_lossy(&repeated.stdout);
    assert!(stdout.contains("DXIL files   0 written"), "{stdout}");
    assert!(stdout.contains("without overwriting"), "{stdout}");
    assert_eq!(fs::read_dir(&output).unwrap().count(), 2);
    for (path, bytes) in files {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}
