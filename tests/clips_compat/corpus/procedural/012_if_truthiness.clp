;; Zero and STRING FALSE select the true branch.
;; Level: boundary
;; Covers: if
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (if 0 then (printout t "zero-true" crlf))
    (if "FALSE" then (printout t "string-true" crlf)))
