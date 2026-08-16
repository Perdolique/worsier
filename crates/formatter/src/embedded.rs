use std::ops::Range;

use oxc_span::SourceType;

use crate::FormatError;

#[derive(Debug)]
pub(crate) struct EmbeddedRegion {
    pub(crate) range: Range<usize>,
    pub(crate) source_type: SourceType,
    pub(crate) label: String,
}

pub(crate) fn validate_regions(
    source: &str,
    regions: &[EmbeddedRegion],
) -> Result<(), FormatError> {
    let mut previous_end = 0;
    for region in regions {
        if region.range.start < previous_end
            || region.range.start > region.range.end
            || region.range.end > source.len()
            || !source.is_char_boundary(region.range.start)
            || !source.is_char_boundary(region.range.end)
        {
            return Err(FormatError::internal(
                "embedded adapter returned invalid or overlapping source ranges",
            ));
        }
        previous_end = region.range.end;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{EmbeddedRegion, validate_regions};

    #[test]
    fn rejects_invalid_adapter_ranges() {
        let source = "aéz";
        let assert_invalid = |ranges: &[std::ops::Range<usize>]| {
            let regions = ranges
                .iter()
                .cloned()
                .map(|range| EmbeddedRegion {
                    range,
                    source_type: oxc_span::SourceType::mjs(),
                    label: "test".to_owned(),
                })
                .collect::<Vec<_>>();
            assert_eq!(
                validate_regions(source, &regions).unwrap_err().code(),
                "INTERNAL_ERROR"
            );
        };
        assert_invalid(std::slice::from_ref(&(1..2)));
        assert_invalid(std::slice::from_ref(&(0..5)));
        assert_invalid(std::slice::from_ref(&std::ops::Range { start: 2, end: 1 }));
        assert_invalid(&[0..3, 1..4]);
    }
}
