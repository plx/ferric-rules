;; Nth retrieves first and last fields with one-based indices.
;; Level: basic
;; Covers: create$, nth$
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (nth$ 1 (create$ a b c)) " " (nth$ 3 (create$ a b c)) crlf))
