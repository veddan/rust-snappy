pub fn next_power_of_2(n: u32) -> u32 {
    let mut v = n.wrapping_sub(1);
    v |= v >> 1;
    v |= v >> 2;
    v |= v >> 4;
    v |= v >> 8;
    v |= v >> 16;
    v.wrapping_add(1)
}
