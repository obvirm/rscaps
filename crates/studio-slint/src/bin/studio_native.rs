//! Entry point native desktop (fitur "native"). LIHAT AGENTS.md.

fn main() {
    studio_slint::platform::native::run().expect("studio-native gagal jalan");
}
