fn assert_send<T: Send>() {}

fn main() {
    assert_send::<acadsharp_rs::Document>();
}
