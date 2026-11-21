// xrustc stage 0 must reject this file with a line number instead of
// compiling it: the native backend has no divide instruction.
fn main() -> i64 {
    let value: i64 = 10 / 2;
    return value;
}
