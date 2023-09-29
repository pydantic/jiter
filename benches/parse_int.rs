#![feature(test)]

extern crate test;

use test::{black_box, Bencher};

fn parse_8(b: &[u8]) -> i64 {
    let p = b.as_ptr() as *const _;
    let mut chunk = 0;
    unsafe {
        std::ptr::copy_nonoverlapping(p, &mut chunk, 8);
    }

    // 1-byte mask trick (works on 4 pairs of single digits)
    let lower_digits = (chunk & 0x0f000f000f000f00) >> 8;
    let upper_digits = (chunk & 0x000f000f000f000f) * 10;
    let chunk = lower_digits + upper_digits;

    // 2-byte mask trick (works on 2 pairs of two digits)
    let lower_digits = (chunk & 0x00ff000000ff0000) >> 16;
    let upper_digits = (chunk & 0x000000ff000000ff) * 100;
    let chunk = lower_digits + upper_digits;

    // 4-byte mask trick (works on a pair of four digits)
    let lower_digits = (chunk & 0x0000ffff00000000) >> 32;
    let upper_digits = (chunk & 0x000000000000ffff) * 10000;
    let chunk = lower_digits + upper_digits;

    chunk
}

fn parse_4(b: &[u8]) -> i64 {
    let p = b.as_ptr() as *const _;
    let mut chunk = 0;
    unsafe {
        std::ptr::copy_nonoverlapping(p, &mut chunk, 4);
    }

    chunk <<= 4 * 8;

    // 1-byte mask trick (works on 4 pairs of single digits)
    let lower_digits = (chunk & 0x0f000f000f000f00) >> 8;
    let upper_digits = (chunk & 0x000f000f000f000f) * 10;
    let chunk = lower_digits + upper_digits;

    // 2-byte mask trick (works on 2 pairs of two digits)
    let lower_digits = (chunk & 0x00ff000000ff0000) >> 16;
    let upper_digits = (chunk & 0x000000ff000000ff) * 100;
    let chunk = lower_digits + upper_digits;

    (chunk & 0x0000ffff00000000) >> 32
}

fn parse_3(b: &[u8]) -> i64 {
    let (left, right) = b.split_at(1);
    return parse_1(left) * 100 + parse_2(right);
}

fn parse_2(b: &[u8]) -> i64 {
    let p = b.as_ptr() as *const _;
    let mut chunk = 0;
    unsafe {
        std::ptr::copy_nonoverlapping(p, &mut chunk, 2);
    }

    dbg!(format!("{:#x?}", chunk));
    // shift the chunk to the left by 6 bytes
    chunk <<= (8 - 2) * 8;
    dbg!(format!("{:#x?}", chunk));

    // 1-byte mask trick (works on 4 pairs of single digits)
    let lower_digits = (chunk & 0x0f000f000f000f00) >> 8;
    let upper_digits = (chunk & 0x000f000f000f000f) * 10;
    let chunk = lower_digits + upper_digits;

    // 2-byte mask trick (works on 2 pairs of two digits)
    (chunk & 0x00ff000000ff0000) >> 48
}

fn parse_1(b: &[u8]) -> i64 {
    let digit = b[0];
    let digit_int = digit & 0x0f;
    digit_int as i64
}

pub fn parse_16(b: &[u8]) -> i64 {
    match b.len() {
        // b can't be shorter than 4
        4 => return parse_4(b),
        5 => {
            let (left, right) = b.split_at(4);
            return parse_4(left) * 10 + parse_1(right);
        }
        6 => {
            let (left, right) = b.split_at(4);
            return parse_4(left) * 100 + parse_2(right);
        }
        7 => {
            let (left, right) = b.split_at(4);
            return parse_4(left) * 1_000 + parse_3(right);
        }
        8 => return parse_8(b),
        9 => {
            let (left, right) = b.split_at(8);
            return parse_8(left) * 10 + parse_1(right);
        }
        10 => {
            let (left, right) = b.split_at(8);
            return parse_8(left) * 100 + parse_2(right);
        }
        11 => {
            let (left, right) = b.split_at(8);
            return parse_8(left) * 1_000 + parse_3(right);
        }
        12 => {
            let (left, right) = b.split_at(8);
            return parse_8(left) * 10_000 + parse_4(right);
        }
        13 => {
            let (left, right) = b.split_at(8);
            let (right_left, right_right) = right.split_at(4);
            return parse_8(left) * 100_000 + parse_4(right_left) * 10 + parse_1(right_right);
        }
        14 => {
            let (left, right) = b.split_at(8);
            let (right_left, right_right) = right.split_at(4);
            return parse_8(left) * 1_000_000 + parse_4(right_left) * 100 + parse_2(right_right);
        }
        15 => {
            let (left, right) = b.split_at(8);
            let (right_left, right_right) = right.split_at(4);
            return parse_8(left) * 10_000_000 + parse_4(right_left) * 1_000 + parse_3(right_right);
        }
        16 => {
            let (left, right) = b.split_at(8);
            return parse_8(left) * 100_000_000 + parse_8(right);
        }
        _ => panic!("too long"),
    }
}

fn parse_fast(b: &[u8]) -> i64 {
    let mut value: i64 = match b.get(0) {
        Some(next) if (b'0'..=b'9').contains(next) => (next & 0x0f) as i64,
        _ => panic!("not a digit"),
    };
    for index in 1..4 {
        match b.get(index) {
            Some(next) if (b'0'..=b'9').contains(next) => {
                value = value * 10 + (next & 0x0f) as i64;
            }
            _ => return value,
        }
    }
    let mut index = 4;
    while let Some(next) = b.get(index) {
        match next {
            b'0'..=b'9' => (),
            _ => return parse_16(&b[0..index]),
        }
        index += 1;
    }
    parse_16(&b[0..index])
}

fn simple(b: &[u8]) -> i64 {
    let mut index: usize = 0;
    let mut value: i64 = 0;
    while let Some(next) = b.get(index) {
        match next {
            b'0'..=b'9' => {
                let digit_int = next & 0x0f;
                if let Some(mult_10) = value.checked_mul(10) {
                    if let Some(add_digit) = mult_10.checked_add((digit_int) as i64) {
                        value = add_digit;
                    } else {
                        panic!("overflow");
                    }
                } else {
                    panic!("overflow");
                }
            }
            _ => return value,
        }
        index += 1;
    }
    value
}

#[bench]
fn one_to_16_fast(bench: &mut Bencher) {
    assert_eq!(parse_fast(b"1"), 1);
    assert_eq!(parse_fast(b"12"), 12);
    assert_eq!(parse_fast(b"123"), 123);
    assert_eq!(parse_fast(b"1234"), 1234);
    assert_eq!(parse_fast(b"12345"), 12345);
    assert_eq!(parse_fast(b"123456"), 123456);
    assert_eq!(parse_fast(b"1234567"), 1234567);
    assert_eq!(parse_fast(b"12345678"), 12345678);
    assert_eq!(parse_fast(b"123456789"), 123456789);
    assert_eq!(parse_fast(b"1234567890"), 1234567890);
    assert_eq!(parse_fast(b"12345678901"), 12345678901);
    assert_eq!(parse_fast(b"123456789012"), 123456789012);
    assert_eq!(parse_fast(b"1234567890123"), 1234567890123);
    assert_eq!(parse_fast(b"12345678901234"), 12345678901234);
    assert_eq!(parse_fast(b"123456789012345"), 123456789012345);
    assert_eq!(parse_fast(b"1234567890123456"), 1234567890123456);

    bench.iter(|| {
        let mut v = parse_fast(black_box(b"1"));
        v += parse_fast(black_box(b"12"));
        v += parse_fast(black_box(b"123"));
        v += parse_fast(black_box(b"1234"));
        v += parse_fast(black_box(b"12345"));
        v += parse_fast(black_box(b"123456"));
        v += parse_fast(black_box(b"1234567"));
        v += parse_fast(black_box(b"12345678"));
        v += parse_fast(black_box(b"123456789"));
        v += parse_fast(black_box(b"1234567890"));
        v += parse_fast(black_box(b"12345678901"));
        v += parse_fast(black_box(b"123456789012"));
        v += parse_fast(black_box(b"1234567890123"));
        v += parse_fast(black_box(b"12345678901234"));
        v += parse_fast(black_box(b"123456789012345"));
        v += parse_fast(black_box(b"1234567890123456"));
        black_box(v)
    })
}

#[bench]
fn one_to_16_simple(bench: &mut Bencher) {
    bench.iter(|| {
        let mut v = simple(black_box(b"1"));
        v += simple(black_box(b"12"));
        v += simple(black_box(b"123"));
        v += simple(black_box(b"1234"));
        v += simple(black_box(b"12345"));
        v += simple(black_box(b"123456"));
        v += simple(black_box(b"1234567"));
        v += simple(black_box(b"12345678"));
        v += simple(black_box(b"123456789"));
        v += simple(black_box(b"1234567890"));
        v += simple(black_box(b"12345678901"));
        v += simple(black_box(b"123456789012"));
        v += simple(black_box(b"1234567890123"));
        v += simple(black_box(b"12345678901234"));
        v += simple(black_box(b"123456789012345"));
        v += simple(black_box(b"1234567890123456"));
        black_box(v)
    })
}

#[bench]
fn size_3_fast(bench: &mut Bencher) {
    bench.iter(|| {
        black_box(parse_fast(black_box(b"123")));
    })
}

#[bench]
fn size_3_simple(bench: &mut Bencher) {
    bench.iter(|| {
        black_box(simple(black_box(b"123")));
    })
}

#[bench]
fn size_4_fast(bench: &mut Bencher) {
    bench.iter(|| {
        black_box(parse_fast(black_box(b"1234")));
    })
}

#[bench]
fn size_4_simple(bench: &mut Bencher) {
    bench.iter(|| {
        black_box(simple(black_box(b"1234")));
    })
}

#[bench]
fn size_7_fast(bench: &mut Bencher) {
    bench.iter(|| {
        black_box(parse_fast(black_box(b"1234567")));
    })
}

#[bench]
fn size_7_simple(bench: &mut Bencher) {
    bench.iter(|| {
        black_box(simple(black_box(b"1234567")));
    })
}

#[bench]
fn size_8_fast(bench: &mut Bencher) {
    bench.iter(|| {
        black_box(parse_fast(black_box(b"12345678")));
    })
}

#[bench]
fn size_8_simple(bench: &mut Bencher) {
    bench.iter(|| {
        black_box(simple(black_box(b"12345678")));
    })
}

#[bench]
fn size_16_fast(bench: &mut Bencher) {
    bench.iter(|| {
        black_box(parse_16(black_box(b"1234567812345678")));
    })
}

#[bench]
fn size_16_simple(bench: &mut Bencher) {
    bench.iter(|| {
        black_box(simple(black_box(b"1234567812345678")));
    })
}
