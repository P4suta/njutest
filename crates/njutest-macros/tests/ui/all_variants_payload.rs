#[derive(njutest_macros::AllVariants)]
enum CarriesData {
    Unit,
    Tuple(u8),
}

fn main() {}
