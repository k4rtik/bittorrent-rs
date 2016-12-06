extern crate syntex;
extern crate pnet_macros;

use std::env;
use std::path::Path;

fn main() {
    let mut registry = syntex::Registry::new();
    pnet_macros::register(&mut registry);

    let src = Path::new("src/packet/peer_pkt.rs.in");
    let dst = Path::new(&env::var_os("OUT_DIR").unwrap()).join("peer_pkt.rs");

    registry.expand("", &src, &dst).unwrap();
}
