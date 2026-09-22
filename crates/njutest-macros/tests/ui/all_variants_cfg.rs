#[derive(njutest_macros::AllVariants)]
enum Conditional {
    Always,
    #[cfg(not(test))]
    Platform,
}

fn main() {}
