//! Fuzzes the key-less entry points: header decoding, `probe` (header +
//! prelude parsing) and the `.darmkey` parser.
#![no_main]

use darm_package::{Header, KeyFile, probe};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(header) = Header::decode(data) {
        // A header that decodes must re-encode to the same bytes.
        assert_eq!(&header.encode()[..], &data[..darm_package::HEADER_LEN]);
        assert!(header.head_len() <= header.total_len);
    }
    if let Ok(info) = probe(data) {
        assert!(info.head_len <= info.total_len);
        let _ = info.version();
        let _ = info.uuid_string();
    }
    if let Ok(text) = std::str::from_utf8(data) {
        if let Ok(keyfile) = KeyFile::parse(text) {
            // A key file that parses must survive a round trip.
            let again = KeyFile::parse(&keyfile.to_text()).expect("round trip");
            assert_eq!(again.key(), keyfile.key());
        }
    }
});
