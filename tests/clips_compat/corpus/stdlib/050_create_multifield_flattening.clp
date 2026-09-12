;; Create flattens nested multifield arguments without changing element order.
;; Level: interaction
;; Covers: create$
;; Run with load, reset, and run in a fresh environment.

(deffacts startup (go))

(defrule exercise
    (go)
    =>
    (printout t (create$ a (create$ b c) (create$) d) crlf))
