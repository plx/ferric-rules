;; Nth returns nil for the zero index.
;; Level: boundary
;; Covers: create$, nth$
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (nth$ 0 (create$ a b)) crlf))
