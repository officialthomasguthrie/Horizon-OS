// horizon-keybuild: build the filesystems of a Horizon Key.
//
// A host-side tool, not part of a running Horizon. It builds the immutable base image
// into an output directory and prints the kernel command line a bootloader passes so
// the init finds the Key. Each --bin installs a host binary into the base's /usr/bin
// with its shared-library closure, so `--bin target/release/horizon --bin
// target/release/horizon-init` makes a base that boots. The build logic is in the
// keybuild library, tested there; this is the thin CLI over it.

use std::path::PathBuf;
use std::process::ExitCode;

struct Args {
    out: PathBuf,
    bins: Vec<PathBuf>,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let Some(parsed) = parse_args(&args) else {
        eprintln!("usage: horizon-keybuild --out <dir> [--bin <path>]...");
        return ExitCode::FAILURE;
    };

    let mut spec = keybuild::KeySpec::new(parsed.out);
    spec.userland = parsed.bins;
    match keybuild::build_base(&spec) {
        Ok(path) => {
            println!("built {}", path.display());
            if !spec.userland.is_empty() {
                println!(
                    "userland: {} binary(ies) plus shared-library closure",
                    spec.userland.len()
                );
            }
            println!("boot cmdline: {}", keybuild::boot_cmdline(&spec));
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("horizon-keybuild: {e}");
            ExitCode::FAILURE
        }
    }
}

fn parse_args(args: &[String]) -> Option<Args> {
    let mut out = None;
    let mut bins = Vec::new();
    let mut it = args.iter().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--out" => out = Some(PathBuf::from(it.next()?)),
            "--bin" => bins.push(PathBuf::from(it.next()?)),
            _ => return None,
        }
    }
    Some(Args { out: out?, bins })
}
