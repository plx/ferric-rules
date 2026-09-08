;; Round resolves exact half-integer ties toward negative infinity.
;; Level: boundary
;; Covers: round
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (round 2.5) " " (round -2.5) " " (round 2.49) " " (round -2.49) crlf))
