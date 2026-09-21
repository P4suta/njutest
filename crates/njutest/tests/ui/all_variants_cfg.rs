#[derive(njutest::AllVariants)]
enum Conditional {
    Always,
    #[cfg(unix)]
    Platform,
}

fn main() {}
