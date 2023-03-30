/// Finds the value for the given tail percentile.
///
/// For example, if the 90th percentile is used, the top 10% of values should fall above the value
/// shown.
///
/// # Note
/// This assumes pre-sorted data
pub(crate) fn find_percentile<T>(data: &mut [T], percentile: f64) -> T
where
    T: Ord + PartialOrd + PartialEq + Clone,
{
    data[((data.len() - 1) as f64 * percentile) as usize].clone()
}
