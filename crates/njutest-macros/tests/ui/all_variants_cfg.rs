#[derive(njutest_macros::AllVariants)]
enum Conditional {
    Always,
    #[cfg(unix)]
    Platform,
}

fn main() {}
