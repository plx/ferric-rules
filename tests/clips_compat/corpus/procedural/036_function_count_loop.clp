;; Function loop bodies can accumulate a global and return the final value.
;; Level: interaction
;; Covers: +, bind, deffunction, defglobal, loop-for-count, sum-to
;; Run with load, reset, and run in a fresh environment.

(defglobal ?*sum* = 0)
(deffunction sum-to (?n)
    (bind ?*sum* 0)
    (loop-for-count (?i 1 ?n) do (bind ?*sum* (+ ?*sum* ?i)))
    ?*sum*)

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (sum-to 4) crlf))
