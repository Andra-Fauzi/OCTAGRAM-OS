// xrustc stage 0 example: recursion, loops, string and character output.
// Build it inside OCTAGRAM with:
//   xrustc /share/octagram/xrustc-sum.rs -o /tmp/xrustc-sum.elf
//   run /tmp/xrustc-sum.elf
fn fib(n: i64) -> i64 {
    if n < 2 {
        return n;
    }
    return fib(n - 1) + fib(n - 2);
}

// Stage 0 has no divide instruction, so print a value below 100 by
// subtracting ten at a time.
fn print_two_digits(value: i64) -> i64 {
    let mut rest: i64 = value;
    let mut tens: i64 = 0;
    while rest >= 10 {
        rest = rest - 10;
        tens = tens + 1;
    }
    if tens > 0 {
        putchar(48 + tens);
    }
    putchar(48 + rest);
    return 0;
}

fn main() -> i64 {
    let mut index: i64 = 0;
    let mut sum: i64 = 0;
    while index < 10 {
        sum = sum + fib(index);
        index = index + 1;
    }
    putstr("xrustc fib sum: ");
    print_two_digits(sum);
    putchar(10);
    return 0;
}
