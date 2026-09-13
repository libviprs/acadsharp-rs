//! Who closes what, and in which order.
//!
//! `viprs_acad_close` releases every decode handle still open on the document,
//! so a document freed while a stream is alive is a use after free the library
//! would refuse rather than fault. The compile-fail case in
//! `tests/api_compile_fail.rs` is what makes that unrepresentable; this file
//! is the other half, which counts the calls that actually went out.
//!
//! There is exactly one `#[test]` in this file on purpose. The counters are
//! process-wide, and a second test thread opening a document inside the
//! measurement window would make every delta below mean nothing. One test per
//! binary is how that is guaranteed, since cargo gives each integration test
//! file its own process.
#![cfg(acadsharp_linked)]

use acadsharp_rs::{Decoder, Document, Limits, diagnostics};

const SYNTHETIC: &[u8] = b"VIPRSSYN\x01\x00\x00\x00\x03\x00\x00\x00";

#[test]
fn every_handle_is_closed_exactly_once_and_the_decode_goes_first() {
    let decoder = Decoder::new().expect("the handshake passes");

    let documents_before = diagnostics::document_closes();
    let decodes_before = diagnostics::decode_closes();

    {
        let document =
            Document::open_bytes(&decoder, SYNTHETIC, &Limits::new()).expect("it opens");
        assert_eq!(
            diagnostics::document_closes(),
            documents_before,
            "opening a document closes nothing"
        );

        {
            let mut stream = document.decode(0).expect("it decodes");
            assert!(stream.next().is_some(), "the stream produced an item");
            assert_eq!(
                diagnostics::decode_closes(),
                decodes_before,
                "a live stream has not been closed"
            );
        }

        assert_eq!(
            diagnostics::decode_closes(),
            decodes_before + 1,
            "dropping the stream closes its decode handle exactly once"
        );
        assert_eq!(
            diagnostics::document_closes(),
            documents_before,
            "dropping a stream does not close the document it borrowed"
        );

        // A second stream on the same document is legal: ABI.md allows two
        // decode handles on one document, including from two threads.
        {
            let _second = document.decode(0).expect("a second decode handle");
        }
        assert_eq!(diagnostics::decode_closes(), decodes_before + 2);
    }

    assert_eq!(
        diagnostics::document_closes(),
        documents_before + 1,
        "dropping the document closes it exactly once"
    );
    assert_eq!(
        diagnostics::decode_closes(),
        decodes_before + 2,
        "and closes no decode handle of its own, because both were already gone"
    );

    assert!(
        diagnostics::last_decode_close() < diagnostics::last_document_close(),
        "the decode handle has to be released before the document it came from, and the \
         sequence numbers say decode {} then document {}",
        diagnostics::last_decode_close(),
        diagnostics::last_document_close()
    );

    // A stream that is still alive when its own scope ends closes on the way
    // out too, so a caller who never finishes a walk leaks nothing.
    let documents_mid = diagnostics::document_closes();
    let decodes_mid = diagnostics::decode_closes();
    {
        let document =
            Document::open_bytes(&decoder, SYNTHETIC, &Limits::new()).expect("it opens");
        let mut stream = document.decode(0).expect("it decodes");
        assert!(stream.next().is_some());
        // Both go out of scope here, the stream first, because a value
        // declared later is dropped first.
    }
    assert_eq!(diagnostics::decode_closes(), decodes_mid + 1);
    assert_eq!(diagnostics::document_closes(), documents_mid + 1);
    assert!(diagnostics::last_decode_close() < diagnostics::last_document_close());
}
