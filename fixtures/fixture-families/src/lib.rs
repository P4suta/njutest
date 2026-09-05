// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One site for every v1 rule, plus the shapes the walker has to get right: nested modules, methods, closures, match arms, multi-line conditions.

pub fn literals(flag: bool) -> bool {
    let a = true;
    let b = false;
    a && b || flag
}

pub fn conditions(x: i32, y: i32) -> i32 {
    let mut total = 0;
    if x < y && !(x == 0) {
        total += 1;
    }
    while total < 10 {
        total += 2;
    }
    if x <= y
        || y >= 0
    {
        total -= 1;
    }
    if x != y || x > y {
        total *= 2;
    }
    total
}

pub fn arithmetic(a: i32, b: i32) -> i32 {
    (a + b) * (a - b) / (b % 3) + (a & b) - (a | b) + (a ^ b) - (a << 1) + (b >> 1)
}

pub fn ranges(v: &[u8]) -> usize {
    let mut n = 0;
    for i in 0..v.len() {
        n += usize::from(v[i]);
    }
    for i in 1..=3 {
        n -= i;
    }
    v[1..].len() + v[..=2].len() + n
}

pub fn results(s: &str) -> Result<i32, std::num::ParseIntError> {
    let n = s.parse::<i32>()?;
    s.parse::<i32>()?;
    if n < 0 {
        return Ok(0);
    }
    Ok(n)
}

pub fn options(s: &str) -> Option<usize> {
    let first = s.chars().next()?;
    Some(first.len_utf8())
}

pub fn deletions(v: &mut Vec<i32>) {
    let mut x = 1;
    v.push(x);
    x = 2;
    x += 3;
    v.push(x);
    v.clear();
}

pub mod inner {
    pub struct Counter {
        pub n: u32,
    }

    impl Counter {
        pub fn bump(&mut self, by: u32) -> bool {
            self.n += by;
            self.n > 10
        }

        pub fn classify(&self) -> &'static str {
            match self.n {
                0 => "zero",
                n if n < 5 => "few",
                _ => "many",
            }
        }
    }

    pub fn closures(xs: &[i32]) -> Vec<i32> {
        xs.iter().map(|x| x * 2).filter(|x| *x > 2).collect()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn t() {
        assert!(super::literals(true));
    }
}
