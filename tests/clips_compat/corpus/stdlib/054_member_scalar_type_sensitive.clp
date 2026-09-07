;; Member returns the first index and distinguishes numeric operand types.
;; Level: boundary
;; Covers: create$, member$
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (member$ b (create$ a b b)) " " (member$ 2.0 (create$ 2)) " " (member$ z (create$ a b)) crlf))
