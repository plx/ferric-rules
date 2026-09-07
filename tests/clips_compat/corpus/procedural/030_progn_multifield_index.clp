;; Progn$ binds each element and its one-based index.
;; Level: interaction
;; Covers: create$, progn$
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (progn$ (?item (create$ a b c)) (printout t ?item-index ":" ?item crlf)))
