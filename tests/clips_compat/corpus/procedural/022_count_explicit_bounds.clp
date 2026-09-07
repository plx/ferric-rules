;; Loop-for-count includes both explicit bounds.
;; Level: boundary
;; Covers: loop-for-count
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (loop-for-count (?i -1 1) do (printout t ?i crlf)))
