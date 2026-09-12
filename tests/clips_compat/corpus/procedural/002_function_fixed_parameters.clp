;; Function parameters bind independently for repeated calls.
;; Level: interaction
;; Covers: -, deffunction, difference
;; Run with load, reset, and run in a fresh environment.

(deffunction difference (?left ?right) (- ?left ?right))

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (difference 9 2) " " (difference 2 9) crlf))
