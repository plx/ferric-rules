;; Integer conversion truncates toward zero and FLOAT conversion preserves value.
;; Level: basic
;; Covers: float, integer
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (integer 3.9) " " (integer -3.9) " " (float 3) crlf))
