;; First and rest return MULTIFIELD, including on empty input.
;; Level: boundary
;; Covers: create$, first$, rest$
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (first$ (create$ a b)) " " (rest$ (create$ a b)) " " (first$ (create$)) " " (rest$ (create$)) crlf))
