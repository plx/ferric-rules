;; Parity predicates handle zero and negative integers.
;; Level: boundary
;; Covers: evenp, oddp
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (evenp 0) " " (oddp 0) " " (evenp -2) " " (oddp -3) crlf))
