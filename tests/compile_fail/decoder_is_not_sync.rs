fn assert_sync<T: Sync>() {}

fn main() {
    assert_sync::<acadsharp_rs::Decoder>();
}
