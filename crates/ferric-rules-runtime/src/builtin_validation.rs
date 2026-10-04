//! Source-time argument checks for built-ins, before a construct is published.
//!
//! Restrictions mirror CLIPS 6.30's `get-function-restrictions`: the first
//! characters give minimum/maximum arity (`*` means unbounded), followed by a
//! default argument type and optional positional overrides. Unknown expression
//! results remain runtime checks; this does not execute or infer user functions.

use ferric_rules_parser::{ActionExpr, FunctionCall, LiteralKind};

fn restrictions(name: &str) -> Option<&'static [u8]> {
    Some(match name {
        "+" | "-" | "*" | "/" | "div" | "min" | "max" | ">" | "<" | ">=" | "<=" | "=" | "!="
        | "<>" => b"2*n",
        "mod" | "**" | "atan2" => b"22n",
        "abs" | "integer" | "float" | "sqrt" | "sin" | "cos" | "tan" | "asin" | "acos" | "atan"
        | "sinh" | "cosh" | "tanh" | "asinh" | "acosh" | "atanh" | "exp" | "log" | "log10"
        | "round" | "ceiling" | "floor" | "deg-rad" | "rad-deg" | "deg-grad" | "grad-deg" => b"11n",
        "eq" | "neq" | "and" | "or" => b"2*",
        "not"
        | "integerp"
        | "floatp"
        | "numberp"
        | "symbolp"
        | "stringp"
        | "lexemep"
        | "instance-namep"
        | "multifieldp"
        | "set-fact-duplication" => b"11",
        "symbol-to-instance-name"
        | "undefrule"
        | "ppdefrule"
        | "deftemplate-slot-names"
        | "set-strategy" => b"11w",
        "instance-name-to-symbol" => b"11p",
        "evenp" | "oddp" | "setgen" | "seed" => b"11i",
        "str-cat" | "sym-cat" | "printout" => b"1*",
        "progn" => b"0*",
        "gensym"
        | "gensym*"
        | "get-fact-duplication"
        | "get-strategy"
        | "get-focus"
        | "get-focus-stack"
        | "pi"
        | "halt"
        | "reset"
        | "clear"
        | "time"
        | "next-methodp"
        | "call-next-method" => b"00",
        "watch" | "unwatch" | "sort" => b"1**w",
        "str-length" | "upcase" | "lowcase" | "string-to-field" => b"11j",
        "sub-string" => b"33*iij",
        "create$"
        | "return"
        | "break"
        | "bind"
        | "assert"
        | "modify"
        | "duplicate"
        | "override-next-method" => b"0**",
        "length" | "length$" => b"11q",
        "subseq$" | "delete$" => b"33im",
        "nth" | "nth$" => b"22*im",
        "implode$" | "first$" | "rest$" | "expand$" => b"11m",
        "member" | "member$" => b"22*um",
        "subsetp" => b"22*mm",
        "format" => b"2**us",
        "read" | "readline" | "close" => b"01",
        "load" | "load-facts" | "eval" | "build" => b"11k",
        "assert-string" | "str-assert" | "explode$" | "str-explode" => b"11s",
        "rules"
        | "refresh-agenda"
        | "get-deftemplate-list"
        | "get-defglobal-list"
        | "get-defrule-list" => b"01w",
        "str-index" => b"22j",
        "str-compare" => b"23*jji",
        "insert$" => b"3**mi",
        "replace$" => b"4**mii",
        "delete-member$" => b"2**m",
        "replace-member$" => b"3**m",
        "funcall" => b"1**k",
        "call-specific-method" => b"2**wi",
        "deftemplate-slot-allowed-values"
        | "deftemplate-slot-multip"
        | "deftemplate-slot-singlep"
        | "deftemplate-slot-types"
        | "deftemplate-slot-default-value"
        | "deftemplate-slot-defaultp"
        | "deftemplate-slot-existp"
        | "deftemplate-slot-range"
        | "deftemplate-slot-cardinality" => b"22w",
        "retract" => b"1*z",
        "focus" => b"1*w",
        "fact-existp" | "fact-relation" | "fact-slot-names" => b"11z",
        "fact-index" => b"11y",
        "fact-slot-value" => b"22*zw",
        "save-facts" => b"1*wk",
        "random" => b"02i",
        _ => return None,
    })
}

fn literal_matches(value: &LiteralKind, restriction: u8) -> bool {
    match restriction {
        b'n' => matches!(value, LiteralKind::Integer(_) | LiteralKind::Float(_)),
        b'i' | b'z' => matches!(value, LiteralKind::Integer(_)),
        b'w' => matches!(value, LiteralKind::Symbol(_)),
        b's' => matches!(value, LiteralKind::String(_)),
        b'p' => matches!(value, LiteralKind::InstanceName(_)),
        b'j' => matches!(
            value,
            LiteralKind::Symbol(_) | LiteralKind::String(_) | LiteralKind::InstanceName(_)
        ),
        b'k' | b'q' => matches!(value, LiteralKind::Symbol(_) | LiteralKind::String(_)),
        b'm' | b'y' => false,
        _ => true,
    }
}

fn type_description(restriction: u8) -> &'static str {
    match restriction {
        b'n' => "integer or float",
        b'i' => "integer",
        b'w' => "symbol",
        b's' => "string",
        b'p' => "instance name",
        b'j' => "symbol, string, or instance name",
        b'k' => "symbol or string",
        b'q' => "multifield, symbol, or string",
        b'm' => "multifield",
        b'y' => "fact address",
        b'z' => "fact address or integer",
        _ => "value",
    }
}

pub(crate) fn validate_call(call: &FunctionCall) -> Result<(), String> {
    if call.name == "set-strategy" {
        if let [ActionExpr::Literal(literal)] = call.args.as_slice() {
            if let LiteralKind::Symbol(name) = &literal.value {
                if !matches!(name.as_str(), "depth" | "breadth" | "lex" | "mea") {
                    return Err(format!(
                        "set-strategy does not support `{name}`; expected depth, breadth, lex, or mea"
                    ));
                }
            }
        }
    }
    let Some(restrictions) = restrictions(&call.name) else {
        return Ok(());
    };
    // An expansion determines the resulting count and positional argument types
    // at execution. Its own operand is still checked by the recursive caller.
    if call
        .args
        .iter()
        .any(|arg| matches!(arg, ActionExpr::FunctionCall(inner) if inner.name == "expand$"))
    {
        return Ok(());
    }
    validate_arity(&call.name, call.args.len(), restrictions)?;
    let default = restrictions.get(2).copied().unwrap_or(b'*');
    for (index, argument) in call.args.iter().enumerate() {
        let ActionExpr::Literal(literal) = argument else {
            continue;
        };
        let restriction = restrictions.get(index + 3).copied().unwrap_or(default);
        if !literal_matches(&literal.value, restriction) {
            return Err(format!(
                "[ARGACCES5] Function {} expected argument #{} to be of type {}",
                call.name,
                index + 1,
                type_description(restriction)
            ));
        }
    }
    Ok(())
}

/// Recheck arity after explicit sequence expansion or dynamic funcall dispatch.
pub(crate) fn validate_runtime_arity(name: &str, count: usize) -> Result<(), String> {
    let Some(restrictions) = restrictions(name) else {
        return Ok(());
    };
    validate_arity(name, count, restrictions)
}

fn validate_arity(name: &str, count: usize, restrictions: &[u8]) -> Result<(), String> {
    let minimum = usize::from(restrictions[0] - b'0');
    let maximum = (restrictions[1] != b'*').then(|| usize::from(restrictions[1] - b'0'));
    if count < minimum || maximum.is_some_and(|maximum| count > maximum) {
        let expected = match maximum {
            Some(maximum) if minimum == maximum => format!("exactly {minimum}"),
            Some(maximum) => format!("between {minimum} and {maximum}"),
            None => format!("at least {minimum}"),
        };
        return Err(format!(
            "[ARGACCES4] Function {name} expected {expected} argument(s), got {count}"
        ));
    }
    Ok(())
}
