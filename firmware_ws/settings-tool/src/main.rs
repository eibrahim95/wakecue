//! The lamp's settings block, from the PC, so a lamp with a dead panel or a stubborn menu can still
//! be given a network, a place and a time zone.
//!
//! The block is written by the same encoder the firmware uses — this tool links `lamp-core` — so the
//! two cannot drift apart.
use std::path::Path;
use std::process::ExitCode;

use lamp_core::settings::{Settings, Stored, SECTOR_A, SECTOR_B, SECTOR_LEN, STORED_LEN};

mod text;

const USAGE: &str = "\
the lamp's settings, from the PC

  wakecue-settings show  --from <dump.bin>
  wakecue-settings build --from <dump.bin> --text <settings.text> --out <sector.bin>

  show   print what the two sectors of a dump hold
  build  start from the newest block the dump holds, apply the text file, and write a 4 KB
         image for sector A (0x9000) with the next sequence number. When the text file is
         not there yet, it writes one to edit and stops.

A dump is read off the lamp with:
  espflash read-flash --chip esp32c3 0x9000 0x2000 settings-dump.bin, board in download mode
";

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> Result<ExitCode, String> {
    let (cmd, flags) = split_args(args)?;
    match cmd.as_str() {
        "show" => {
            show(need(&flags, "--from")?)?;
            Ok(ExitCode::SUCCESS)
        }
        "build" => build(&flags),
        "help" | "-h" | "--help" => {
            print!("{USAGE}");
            Ok(ExitCode::SUCCESS)
        }
        other => Err(format!("`{other}` is not a command\n\n{USAGE}")),
    }
}

fn split_args(args: Vec<String>) -> Result<(String, Vec<(String, String)>), String> {
    let mut it = args.into_iter();
    let cmd = it.next().ok_or(USAGE.to_string())?;
    let mut flags = Vec::new();
    while let Some(k) = it.next() {
        if !k.starts_with("--") {
            return Err(format!("`{k}` is not a flag\n\n{USAGE}"));
        }
        let v = it.next().ok_or_else(|| format!("{k} needs a value"))?;
        flags.push((k, v));
    }
    Ok((cmd, flags))
}

fn flag<'a>(flags: &'a [(String, String)], name: &str) -> Option<&'a str> {
    flags.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

fn need<'a>(flags: &'a [(String, String)], name: &str) -> Result<&'a str, String> {
    flag(flags, name).ok_or_else(|| format!("{name} is missing\n\n{USAGE}"))
}

fn read_dump(path: &str) -> Result<(Option<Stored>, Option<Stored>, usize), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let a = bytes.get(..SECTOR_LEN).and_then(Stored::decode);
    let b = bytes.get(SECTOR_LEN..2 * SECTOR_LEN).and_then(Stored::decode);
    Ok((a, b, bytes.len()))
}

fn show(from: &str) -> Result<(), String> {
    let (a, b, len) = read_dump(from)?;
    println!("{from}: {len} bytes, {SECTOR_LEN} per sector");
    for (name, addr, s) in [("A", SECTOR_A, a), ("B", SECTOR_B, b)] {
        match s {
            Some(s) => println!("  sector {name} at 0x{addr:04x}: valid, sequence {}", s.seq),
            None => println!("  sector {name} at 0x{addr:04x}: blank or damaged"),
        }
    }
    match Stored::newest(a, b) {
        Some(newest) => {
            println!("\nthe lamp loads sequence {}\n", newest.seq);
            print!("{}", text::dump(&newest.settings));
        }
        None => println!("\nneither sector holds a block: the lamp loads its defaults"),
    }
    Ok(())
}

fn build(flags: &[(String, String)]) -> Result<ExitCode, String> {
    let text_path = need(flags, "--text")?;
    let out = need(flags, "--out")?;
    let (a, b) = match flag(flags, "--from") {
        Some(p) if Path::new(p).exists() => {
            let (a, b, _) = read_dump(p)?;
            (a, b)
        }
        Some(p) => {
            eprintln!("note: {p} is not there, starting from the defaults");
            (None, None)
        }
        None => {
            eprintln!("note: no --from, starting from the defaults");
            (None, None)
        }
    };

    let newest = Stored::newest(a, b);
    let seq = newest.map_or(1, |s| s.seq.wrapping_add(1));
    let current = newest.map_or_else(Settings::default, |s| s.settings);

    if !Path::new(text_path).exists() {
        std::fs::write(text_path, text::dump(&current))
            .map_err(|e| format!("{text_path}: {e}"))?;
        println!(
            "wrote {text_path} from {}",
            if newest.is_some() { "the block in the dump" } else { "the defaults" }
        );
        println!("edit it, then run this again");
        return Ok(ExitCode::from(2));
    }

    let src = std::fs::read_to_string(text_path).map_err(|e| format!("{text_path}: {e}"))?;
    let mut settings = current;
    text::apply(&mut settings, &text::parse(&src)?)?;

    let mut image = vec![0xFFu8; SECTOR_LEN];
    image[..STORED_LEN].copy_from_slice(&Stored { seq, settings }.encode());
    std::fs::write(out, &image).map_err(|e| format!("{out}: {e}"))?;

    println!("wrote {out}: sector A at 0x{SECTOR_A:04x}, sequence {seq}");
    println!("to put it on the lamp, with the board in download mode:");
    println!("  espflash erase-region --chip esp32c3 0x{SECTOR_A:04x} 0x1000");
    println!("  espflash write-bin --chip esp32c3 --after watchdog-reset 0x{SECTOR_A:04x} {out}");
    Ok(ExitCode::SUCCESS)
}
