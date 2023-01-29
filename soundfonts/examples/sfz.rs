use xsynth_soundfonts::sfz::grammar::{self};

fn main() {
    let path = "/run/media/d/Midis/Soundfonts/test.sfz";
    let str = std::fs::read_to_string(path).unwrap();
    dbg!("Parsing");

    let result = grammar::Root::parse_full(&str);

    match result {
        Ok(val) => println!("{val:#?}"),
        Err(err) => println!("Error: {err}"),
    }
}
