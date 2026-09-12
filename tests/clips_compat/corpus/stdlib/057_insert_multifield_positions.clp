;; Insert supports beginning, middle, end, and flattened inserted multifields.
;; Level: basic
;; Covers: create$, insert$
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (insert$ (create$ b c) 1 a) " " (insert$ (create$ a d) 2 (create$ b c)) " " (insert$ (create$ a b) 3 c) crlf))
