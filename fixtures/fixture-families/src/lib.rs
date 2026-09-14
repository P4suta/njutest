// SPDX-FileCopyrightText: 2026 njutest contributors
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

pub fn assertions(a: i32, b: i32, o: Option<i32>) {
    assert!(a < b);
    assert_eq!(a + 1, b);
    assert_ne!(a, b - 1);
    debug_assert!(a != 0);
    debug_assert_eq!(a * 2, b);
    debug_assert_ne!(a, b);
    assert!(matches!(o, Some(_)), "o was {o:?}");
    assert!(a > 0, "a was {a}");
}

pub fn compounds(mut n: i32, mut bits: u32) -> (i32, u32) {
    n *= 3;
    n /= 2;
    n %= 7;
    bits &= 0xf0;
    bits |= 0x0f;
    bits ^= 0xff;
    bits <<= 2;
    bits >>= 1;
    (n, bits)
}

pub fn negation(x: i32) -> i32 {
    -x
}

pub fn swaps(o: Option<i32>, r: Result<i32, ()>, a: i32, b: i32) -> i32 {
    let mut n = a.max(b) + a.min(b);
    if o.is_some() {
        n += 1;
    }
    if o.is_none() {
        n += 2;
    }
    if r.is_ok() {
        n += 4;
    }
    if r.is_err() {
        n += 8;
    }
    n
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

pub fn sequences(v: &[i32], s: &str) -> i32 {
    let head = v.first().copied().unwrap_or(0);
    let tail = v.last().copied().unwrap_or(0);
    let some = v.iter().skip(1).take(2).sum::<i32>();
    let rest = v.iter().product::<i32>();
    if v.iter().all(|n| *n > 0) && s.contains('x') {
        return head + tail;
    }
    if v.iter().any(|n| *n < 0) || s.starts_with('a') {
        return some;
    }
    rest
}

pub fn parsed(s: &str) -> Result<i32, String> {
    s.parse::<i32>().map_err(|e| e.to_string())
}

pub fn jumps(v: &[i32]) -> i32 {
    let mut n = 0;
    'outer: for x in v {
        for y in v {
            if *y == 0 {
                continue 'outer;
            }
            if *x < 0 {
                break;
            }
            n += *x;
        }
    }
    if n > 100 {
        n = 100;
    } else {
        n += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    #[test]
    fn t() {
        assert!(super::literals(true));
    }
}
