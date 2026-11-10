#![no_main]

use anyzip::Format;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| anyzip_fuzz::check(data, Some(Format::Zip)));
