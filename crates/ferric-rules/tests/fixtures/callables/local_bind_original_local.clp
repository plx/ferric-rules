;; Local bind in a deffunction is visible to later expressions in that invocation.
;; Level: interaction
;; Covers: *, +, bind, calculate, deffunction
;; Run with load, reset, and run in a fresh environment.

(deffunction calculate (?x) (bind ?result (+ ?x 1)) (* ?result 2))

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (calculate 3) " " (calculate 5) crlf))
