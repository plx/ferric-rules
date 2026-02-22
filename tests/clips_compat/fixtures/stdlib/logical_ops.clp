; Logical operations: and, or, not.
(deffacts startup (run-logic))

(defrule logic
    (run-logic)
    =>
    (printout t "and: " (and TRUE TRUE) crlf)
    (printout t "or: " (or FALSE TRUE) crlf)
    (printout t "not: " (not FALSE) crlf)
    (printout t "and-false: " (and TRUE FALSE) crlf))
