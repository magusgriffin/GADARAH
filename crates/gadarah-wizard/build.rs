//! Wizard build script.
//!
//! The wizard's install driver embeds an `app payload` — a zip of the
//! files that should land on the user's machine (gadarah-gui.exe,
//! gadarah.exe, config/, README). CI sets `GADARAH_WIZARD_PAYLOAD` to the
//! absolute path of that zip; we copy it into `OUT_DIR/payload.zip` so
//! `include_bytes!` in `install.rs` resolves at compile time.
//!
//! For dev builds (no env var, or the file doesn't exist) we create an
//! empty zip there instead. The wizard still compiles and runs — it just
//! reports an empty payload and skips the extraction step. No runtime
//! surprises.

use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=GADARAH_WIZARD_PAYLOAD");
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR missing"));
    let dest = out_dir.join("payload.zip");

    if let Some(src) = env::var_os("GADARAH_WIZARD_PAYLOAD") {
        let src = PathBuf::from(src);
        if src.is_file() {
            fs::copy(&src, &dest).expect("failed to copy wizard payload into OUT_DIR");
            println!("cargo:rerun-if-changed={}", src.display());
            return;
        }
    }

    // Empty-zip sentinel: a valid, readable zip with zero entries. Keeps
    // `include_bytes!` happy and lets the installer log "no payload".
    let mut f = fs::File::create(&dest).expect("failed to create empty payload.zip");
    // PK end-of-central-directory record for a zero-entry archive.
    let eocd: [u8; 22] = [
        b'P', b'K', 5, 6, // signature
        0, 0, 0, 0, // disk numbers
        0, 0, 0, 0, // entry counts
        0, 0, 0, 0, // central directory size
        0, 0, 0, 0, // central directory offset
        0, 0, // comment length
    ];
    f.write_all(&eocd).expect("failed to write empty-zip sentinel");
}
