//! Compiles the page's TypeScript into the binary. See `rooms_client`.

fn main() {
    rooms_client::bundle("client");
}
