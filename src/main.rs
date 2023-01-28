use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();
    let url = &args[1];
    dbg!(url);
}
