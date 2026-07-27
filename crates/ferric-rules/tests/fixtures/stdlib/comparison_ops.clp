; Comparison operator functions.
; Note: numeric `=` is not usable as a function call (lexer makes it Token::Equals).
; Use `eq` for symbol/value equality; use `<>` for numeric inequality.
(deffacts startup (run-compare))

(defrule compare
    (run-compare)
    =>
    (printout t "gt: " (> 5 3) crlf)
    (printout t "lt: " (< 2 8) crlf)
    (printout t "gte: " (>= 5 5) crlf)
    (printout t "lte: " (<= 3 5) crlf)
    (printout t "neq-num: " (<> 1 2) crlf)
    (printout t "eq-sym: " (eq hello hello) crlf))
