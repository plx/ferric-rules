;; False if with no else performs no actions.
;; Level: basic
;; Covers: if
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (if FALSE then (printout t "wrong" crlf))
    (printout t "after" crlf))
