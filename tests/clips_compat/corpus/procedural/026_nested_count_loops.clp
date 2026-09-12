;; Nested loop-for-count bindings preserve the outer index.
;; Level: interaction
;; Covers: loop-for-count
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (loop-for-count (?i 1 2) do
        (loop-for-count (?j 1 2) do (printout t ?i ":" ?j crlf))))
