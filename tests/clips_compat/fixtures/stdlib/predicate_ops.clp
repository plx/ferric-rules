; Predicate functions: type testing and equality.
(deffacts startup (test-pred))

(defrule pred-test
    (test-pred)
    =>
    (printout t "int? " (integerp 42) crlf)
    (printout t "float? " (floatp 3.5) crlf)
    (printout t "sym? " (symbolp abc) crlf)
    (printout t "str? " (stringp "hello") crlf)
    (printout t "num? " (numberp 42) crlf)
    (printout t "eq: " (eq abc abc) crlf))
