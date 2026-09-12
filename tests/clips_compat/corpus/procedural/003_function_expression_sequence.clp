;; Only the last expression becomes the function result.
;; Level: interaction
;; Covers: *, +, calculate, deffunction
;; Run with load, reset, and run in a fresh environment.

(deffunction calculate (?x) (+ ?x 1) (* ?x 4))

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (calculate 3) crlf))
