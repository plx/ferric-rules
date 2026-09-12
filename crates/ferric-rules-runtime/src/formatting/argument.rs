//! Borrowed primitive operand view, not a substitute for the runtime Value API.
//! The evaluator constructs it after evaluating and validating ownership.

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum FormatArgument<'a> {
    Integer(i64),
    Float(f64),
    String(&'a [u8]),
    Symbol(&'a [u8]),
    InstanceName(&'a [u8]),
    Other(&'static str),
}

impl FormatArgument<'_> {
    pub(crate) fn type_name(self) -> &'static str {
        match self {
            Self::Integer(_) => "INTEGER",
            Self::Float(_) => "FLOAT",
            Self::String(_) => "STRING",
            Self::Symbol(_) => "SYMBOL",
            Self::InstanceName(_) => "INSTANCE-NAME",
            Self::Other(name) => name,
        }
    }
}
