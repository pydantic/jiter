#![feature(test)]

extern crate test;

use test::{black_box, Bencher};

/// See https://rust-malaysia.github.io/code/2020/07/11/faster-integer-parsing.html#the-divide-and-conquer-insight
/// for explanation of the technique.
/// TODO confirm this works on big endian machines
fn parse_8(b: &[u8]) -> i64 {
    let a: [u8; 8] = b.try_into().unwrap();
    let eight_numbers = i64::from_le_bytes(a);

    // assuming the number is `12345678`
    // the bytes are reversed as we look at them (because we're on LE), so we have `87654321`
    // 8 the less significant digit is first
    // dbg!(format!("{eight_numbers:#018x}"));
    // eight_numbers = 0x38|37|36|35|34|33|32|31

    // take 8, 6, 4, 2, apply mask to get their numeric values and shift them to the right by 1 byte
    let lower: i64 = (eight_numbers & 0x0f000f000f000f00) >> 8;
    // dbg!(format!("{lower:#018x}"));
    // lower = 0x00|08|00|06|00|04|00|02

    // take 7, 5, 3, 1, apply mask to get their numeric values and multiply them by 10
    let upper = (eight_numbers & 0x000f000f000f000f) * 10;
    // dbg!(format!("{upper:#018x}"));
    // upper = 0x46|00|32|00|1e|00|0a|00 = 0x46 is 70 - 7 * 10 ... 0x0a is 10 - 1 * 10
    let four_numbers = lower + upper;
    // dbg!(format!("{four_numbers:#018x}"), four_numbers.to_be_bytes());
    // four_numbers = 0x00|4e|00|38|00|22|00|0c = 0x4e is 78 - 70 + 8 ... we're turned 8 numbers into 8

    // take 78 and 34, apply mask to get their numeric values and shift them to the right by 2 bytes
    let lower = (four_numbers & 0x00ff000000ff0000) >> 16;
    // dbg!(format!("{lower:#018x}"), lower.to_be_bytes());
    // lower = 0x00|00|00|4e|00|00|00|22 - 0x4e is 78, 0x22 is 34

    // take 56 and 12, apply mask to get their numeric values and multiply them by 100
    let upper = (four_numbers & 0x000000ff000000ff) * 100;
    // dbg!(format!("{upper:#018x}"));
    // upper = 0x000015e0|000004b0 - 0x000015e0 is 5600, 0x000004b0 is 1200

    let two_numbers = lower + upper;
    // dbg!(format!("{two_numbers:#018x}"));
    // two_numbers = 0x0000162e|000004d2 - 0x0000162e is 5678, 0x000004d2 is 1234

    // take 5678, apply mask to get it's numeric values and shift it to the right by 4 bytes
    let lower = (two_numbers & 0x0000ffff00000000) >> 32;
    // dbg!(format!("{lower:#018x}"));
    // lower = 0x000000000000162e - in base 10 is 5678

    let upper = (two_numbers & 0x000000000000ffff) * 10000;
    // dbg!(format!("{upper:#018x}"));
    // upper = 0x0000000000bc4b20 - in base 10 is 1234_0000

    // combine to get the result!
    lower + upper
}

fn parse_7(b: &[u8]) -> i64 {
    let (left, right) = b.split_at(4);
    return parse_4(left) * 1_000 + parse_3(right);
}

fn parse_6(b: &[u8]) -> i64 {
    let (left, right) = b.split_at(4);
    return parse_4(left) * 100 + parse_2(right);
}

fn parse_5(b: &[u8]) -> i64 {
    let (left, right) = b.split_at(4);
    return parse_4(left) * 10 + parse_1(right);
}

fn parse_4(b: &[u8]) -> i64 {
    let a: [u8; 4] = b.try_into().unwrap();
    let four_numbers = i32::from_le_bytes(a);

    // assuming the number is `1234`
    // dbg!(format!("{four_numbers:#010x}"));
    // four_numbers = 0x34|33|32|31

    // take 4, 2, apply mask to get their numeric values and shift them to the right by 1 byte
    let lower = (four_numbers & 0x0f000f00) >> 8;
    // dbg!(format!("{lower:#010x}"), lower.to_be_bytes());
    // lower = 0x00|04|00|02

    // take 3, 1, apply mask to get their numeric values and multiply them by 10
    let upper = (four_numbers & 0x000f000f) * 10;
    // dbg!(format!("{upper:#010x}"), lower.to_be_bytes());
    // upper = 0x00|1e|00|0a = 0x1e is 30 - 3 * 10, 0x0a is 10 - 1 * 10

    let two_numbers = lower + upper;
    // dbg!(format!("{two_numbers:#010x}"), lower.to_be_bytes());
    // two_numbers = 0x00|22|00|0c = 0x22 is 34 - 30 + 4, 0x0c is 12 - 10 + 2

    // take 34, apply mask to get it's numeric values and shift it to the right by 2 bytes
    let lower = (two_numbers & 0x00ff0000) >> 16;
    // dbg!(format!("{lower:#010x}"));
    // lower = 0x00|00|00|22 - in base 10 is 34

    let upper = (two_numbers & 0x000000ff) * 100;
    // dbg!(format!("{upper:#010x}"));
    // upper = 0x000004b0 - in base 10 is 1200
    (lower + upper) as i64
}

fn parse_3(b: &[u8]) -> i64 {
    let (left, right) = b.split_at(1);
    return parse_1(left) * 100 + parse_2(right);
}

fn parse_2(b: &[u8]) -> i64 {
    let a: [u8; 2] = b.try_into().unwrap();
    let two_numbers = i16::from_le_bytes(a);

    // assuming the number is `12`
    // take 2, apply mask to get it's numeric values and shift it to the right by 1 byte
    let lower = (two_numbers & 0x0f00) >> 8;

    // take 1, apply mask to get it's numeric values and multiply it by 10
    let upper = (two_numbers & 0x000f) * 10;
    (lower + upper) as i64
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
        5 => return parse_5(b),
        6 => return parse_6(b),
        7 => return parse_7(b),
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
            return parse_8(left) * 100_000 + parse_5(right);
        }
        14 => {
            let (left, right) = b.split_at(8);
            return parse_8(left) * 1_000_000 + parse_6(right);
        }
        15 => {
            let (left, right) = b.split_at(8);
            return parse_8(left) * 10_000_000 + parse_7(right);
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

#[test]
fn test_8() {
    assert_eq!(parse_fast(b"12345678"), 12345678);
}

#[test]
fn test_4() {
    assert_eq!(parse_fast(b"1234"), 1234);
    assert_eq!(parse_fast(b"12345"), 12345);
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
