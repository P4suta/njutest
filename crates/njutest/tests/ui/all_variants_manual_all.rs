#[derive(njutest::AllVariants)]
enum Duplicated {
    One,
    Two,
}

impl Duplicated {
    const ALL: [Self; 2] = [Self::One, Self::Two];
}

fn main() {}
