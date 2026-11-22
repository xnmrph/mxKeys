//! Print what the parser sees in a keymap file: `cargo run -p zmk-config --example inspect -- path.keymap`
use zmk_config::dts::Document;
use zmk_config::keymap::{self, Keymap};

fn main() {
    let path = std::env::args().nth(1).expect("usage: inspect <file.keymap>");
    let src = std::fs::read_to_string(&path).expect("read");
    let doc = Document::parse(src.clone());
    println!("lossless: {}", doc.to_source() == src);
    for d in &doc.diagnostics {
        let line = src[..d.span.start].matches('\n').count() + 1;
        println!("diagnostic line {line}: {}", d.message);
    }
    match Keymap::from_document(&doc) {
        Err(e) => println!("keymap: {e}"),
        Ok(km) => {
            for l in &km.layers {
                let b = l.bindings.as_ref();
                println!(
                    "layer {} {:?}: {} bindings, opaque: {:?}",
                    l.index,
                    l.title(),
                    b.map_or(0, |b| b.bindings.len()),
                    b.and_then(|b| b.opaque_reason.clone())
                );
            }
            println!("definitions: {}", km.definitions.len());
            for d in &km.definitions {
                println!("  {:?} {} {:?}", d.kind, d.node_name, d.compatible);
            }
            println!("combos: {}", keymap::combos(&doc).len());
        }
    }
}
