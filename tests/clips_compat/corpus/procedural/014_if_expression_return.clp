;; Function if returns the last value of the selected expression sequence.
;; Level: basic
;; Covers: choose, deffunction, if
;; Run with load, reset, and run in a fresh environment.

(deffunction choose (?flag) (if ?flag then 1 2 else 3 4))

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (choose TRUE) " " (choose FALSE) crlf))
