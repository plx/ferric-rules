;; Replace splices multiple values into an inclusive range.
;; Level: basic
;; Covers: create$, replace$
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (replace$ (create$ a b c d) 2 3 x y z) crlf))
