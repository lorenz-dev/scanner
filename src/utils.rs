/// Format large numbers with optional delimiter (default: "_")
///
/// # Examples
///
/// ```
/// format_lamports(1000, None); // "1_000"
/// format_lamports(1000, Some(",")); // "1,000"
/// ```
pub fn format_lamports(amount: u64, delimiter: Option<&str>) -> String {
    let deli = delimiter.unwrap_or("_");
    let s: String = amount.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push_str(deli);
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

/// Format amount with decimals, showing only whole numbers
///
/// # Examples
///
/// ```
/// format_with_decimals(10_230_000_000, 9); // "10" (10.23 SOL becomes 10)
/// format_with_decimals(1_000_000, 6); // "1" (1 USDC)
/// ```
pub fn format_with_decimals(amount: u64, decimals: u32) -> String {
    if decimals == 0 {
        return format_lamports(amount, None);
    }
    let divisor = 10u64.pow(decimals);
    let whole = amount / divisor;
    format_lamports(whole, Some(","))
}
