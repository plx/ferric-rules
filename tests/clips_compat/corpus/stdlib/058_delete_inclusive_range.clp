;; Delete removes an inclusive range, including deletion of the full sequence.
;; Level: basic
;; Covers: create$, delete$
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (delete$ (create$ a b c d) 2 3) " " (delete$ (create$ a b) 1 2) crlf))
