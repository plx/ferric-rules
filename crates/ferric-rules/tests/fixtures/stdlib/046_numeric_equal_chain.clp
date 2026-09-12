;; Numeric equality compares all operands in a multiargument call.
;; Level: boundary
;; Covers: =
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (= 2 2.0 2) " " (= 2 2 3) crlf))
