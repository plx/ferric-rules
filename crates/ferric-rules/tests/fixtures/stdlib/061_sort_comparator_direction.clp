;; Sort predicate says whether two arguments should be exchanged.
;; Level: basic
;; Covers: create$, sort
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (sort < (create$ 3 1 2)) " " (sort > (create$ 3 1 2)) crlf))
