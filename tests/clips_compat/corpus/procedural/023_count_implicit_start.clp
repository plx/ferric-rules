;; Loop-for-count defaults its start index to one.
;; Level: basic
;; Covers: loop-for-count
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (loop-for-count (?i 3) do (printout t ?i crlf)))
