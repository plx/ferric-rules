;; Nth returns nil for an index larger than the sequence length.
;; Level: boundary
;; Covers: nth$
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (nth$ 3 (create$ a b)) crlf))
