use acadsharp_rs::{Decoder, Document, Limits};

fn main() {
    let decoder = Decoder::new().unwrap();
    let document = Document::open_bytes(&decoder, b"VIPRSSYN", &Limits::new()).unwrap();
    let stream = document.decode(0).unwrap();

    // `viprs_acad_close` releases every decode handle still open on the
    // document, so this is a use after free. The stream borrows the document,
    // so borrowck refuses it.
    drop(document);

    let _ = stream;
}
