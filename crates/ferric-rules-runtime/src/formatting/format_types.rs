//! Shared directive and output-allowance types for numeric and byte renderers.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct FormatSpec {
    pub(crate) left_align: bool,
    pub(crate) zero_pad: bool,
    pub(crate) width: usize,
    pub(crate) precision: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OutputLimit {
    /// None denotes arithmetic overflow or a rejected formatting stream whose
    /// complete final size was intentionally not computed.
    pub(crate) required: Option<usize>,
    pub(crate) allowed: usize,
}
