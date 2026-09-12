;; Nth returns nil for a negative index.
;; Level: boundary
;; Covers: nth$
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (nth$ -1 (create$ a b)) crlf))
